//! What this folder's ports share: a Bridge to call, keys and federations to
//! put in it, a Bitcoin chain that holds a transaction, and the Bridge's
//! methods called as a transaction calls them.
//!
//! What rskj computes and rustock is being held to (a redeem script, an
//! address, a hash) is computed here from rskj's definition, not by calling
//! rustock's own code for it.

use alloy_primitives::{Address as RskAddress, U256};
use bitcoin::hashes::Hash as _;
use revm::context_interface::journaled_state::account::JournaledAccountTr;
use revm::context_interface::{ContextTr, JournalTr};
use revm::MainContext;
use sha2::Digest as _;

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::{BridgeConstants, BtcNetwork};
use crate::bridge::federation::{serialize_federation_multikey, StoredMember};
use crate::bridge::{
    btc_chain, btc_rules, federation, getters, governance, peg, serialization, storage, tx,
};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::{BridgeTxContext, BRIDGE_ADDR};

/// rskj's `FlyoverTxResponseCodes`, as `registerFastBridgeBtcTransaction`
/// answers them.
pub(super) mod flyover_code {
    pub(in super::super) const ALREADY_PROCESSED: i128 = -302;
    pub(in super::super) const VALUE_ZERO: i128 = -304;
    pub(in super::super) const UTXO_BELOW_MINIMUM: i128 = -305;
    pub(in super::super) const GENERIC_ERROR: i128 = -900;
}

/// One RBTC satoshi in wei.
pub(super) const WEI_PER_SATOSHI: u64 = 10_000_000_000;

/// A Bridge with nothing stored, at RSK block `number`.
pub(super) fn ctx_at(number: u64) -> impl crate::RskContextTr {
    revm::Context::mainnet()
        .with_block(revm::context::BlockEnv {
            number: U256::from(number),
            timestamp: U256::from(1_700_000_000u64),
            ..Default::default()
        })
        .with_chain(crate::raw_storage::RskChainExt::default())
}

/// The RSK block a context is at.
pub(super) fn block_of<CTX: crate::RskContextTr>(ctx: &mut CTX) -> u64 {
    revm::context_interface::Block::number(ctx.block()).to::<u64>()
}

/// The compressed public key of the private key `private`, read as rskj's
/// `BtcECKey.fromPrivate(Hex.decode(..))` reads it: a big-endian number.
pub(super) fn public_key(private: &[u8]) -> [u8; 33] {
    let mut scalar = [0u8; 32];
    scalar[32 - private.len()..].copy_from_slice(private);
    let key = k256::ecdsa::SigningKey::from_slice(&scalar).expect("a private key in range");
    key.verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// `count` keys, from the private keys `first`, `first + 1`, ...
pub(super) fn keys(first: u8, count: u8) -> Vec<[u8; 33]> {
    (first..first + count)
        .map(|private| public_key(&[private]))
        .collect()
}

/// bitcoinj-thin's `ScriptBuilder.createRedeemScript(threshold, keys)`: the
/// keys sorted, and the counts as its `ScriptBuilder.number` writes them, OP_1
/// to OP_15 below 16 and a one-byte push from 16 on, to the 20 keys it takes.
pub(super) fn multisig(keys: &[[u8; 33]], threshold: usize) -> Vec<u8> {
    assert!(keys.len() <= 20 && (1..=keys.len()).contains(&threshold));
    let number = |n: usize| {
        if n < 16 {
            vec![0x50 + n as u8]
        } else {
            vec![1, n as u8]
        }
    };
    let mut sorted = keys.to_vec();
    sorted.sort();
    let mut script = number(threshold);
    for key in &sorted {
        script.push(33);
        script.extend_from_slice(key);
    }
    script.extend(number(sorted.len()));
    script.push(0xae);
    script
}

/// A federation's redeem script, as rskj's `StandardMultiSigFederation` has
/// it: `size / 2 + 1` of its members.
pub(super) fn standard_redeem(keys: &[[u8; 33]]) -> Vec<u8> {
    multisig(keys, keys.len() / 2 + 1)
}

/// A compressed public key written in hex.
pub(super) fn key(hex: &str) -> [u8; 33] {
    hex::decode(hex)
        .expect("a key in hex")
        .try_into()
        .expect("a compressed key")
}

/// The emergency keys of a network's Bridge constants.
pub(super) fn erp_keys_of(config: &BridgeConstants) -> Vec<[u8; 33]> {
    config.erp_fed_pubkeys.iter().map(|hex| key(hex)).collect()
}

/// `network`'s Bridge constants with the emergency keys `erp` and the delay
/// `delay` in place of its own.
pub(super) fn with_erp(network: BridgeConstants, erp: &[[u8; 33]], delay: i64) -> BridgeConstants {
    let hexes: Vec<&'static str> = erp
        .iter()
        .map(|key| &*Box::leak(hex::encode(key).into_boxed_str()))
        .collect();
    BridgeConstants {
        erp_fed_pubkeys: Box::leak(hexes.into_boxed_slice()),
        erp_fed_activation_delay: delay,
        ..network
    }
}

/// A non-standard ERP federation's redeem script as rskj's builders write
/// it and its tests read it (`NonStandardErpFederationsTest
/// .validateErpRedeemScript`): the members' multisig and the emergency
/// multisig, each without its `OP_CHECKMULTISIG`, in `OP_NOTIF <members>
/// OP_ELSE <delay> OP_CHECKSEQUENCEVERIFY OP_DROP <emergency> OP_ENDIF
/// OP_CHECKMULTISIG`. `delay` is the bytes the delay is pushed as.
pub(super) fn non_standard_erp_redeem(
    members: &[[u8; 33]],
    emergency: &[[u8; 33]],
    delay: &[u8],
) -> Vec<u8> {
    let members = standard_redeem(members);
    let emergency = standard_redeem(emergency);
    let mut script = vec![0x64];
    script.extend_from_slice(&members[..members.len() - 1]);
    script.extend_from_slice(&[0x67, delay.len() as u8]);
    script.extend_from_slice(delay);
    script.extend_from_slice(&[0xb2, 0x75]);
    script.extend_from_slice(&emergency[..emergency.len() - 1]);
    script.extend_from_slice(&[0x68, 0xae]);
    script
}

/// A P2SH ERP federation's redeem script, which a P2SH-P2WSH ERP federation
/// has too, as rskj's `P2shErpRedeemScriptBuilder` writes it: `OP_NOTIF
/// <members' multisig> OP_ELSE <delay> OP_CHECKSEQUENCEVERIFY OP_DROP
/// <emergency multisig> OP_ENDIF`, each multisig whole. `delay` is the bytes
/// the delay is pushed as, Java's `BigInteger.toByteArray` reversed.
pub(super) fn p2sh_erp_redeem(
    members: &[[u8; 33]],
    emergency: &[[u8; 33]],
    delay: &[u8],
) -> Vec<u8> {
    let mut script = vec![0x64];
    script.extend(standard_redeem(members));
    script.extend_from_slice(&[0x67, delay.len() as u8]);
    script.extend_from_slice(delay);
    script.extend_from_slice(&[0xb2, 0x75]);
    script.extend(standard_redeem(emergency));
    script.push(0x68);
    script
}

/// RIPEMD160(SHA256(data)).
pub(super) fn hash160(data: &[u8]) -> [u8; 20] {
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}

/// `OP_HASH160 <hash> OP_EQUAL`.
pub(super) fn p2sh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0xa9, 20];
    script.extend_from_slice(hash);
    script.push(0x87);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// `OP_DUP OP_HASH160 <hash> OP_EQUALVERIFY OP_CHECKSIG`.
pub(super) fn p2pkh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0x76, 0xa9, 20];
    script.extend_from_slice(hash);
    script.extend_from_slice(&[0x88, 0xac]);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// `OP_0 <hash>`: a native segwit (bech32) output, P2WPKH.
pub(super) fn p2wpkh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0x00, 20];
    script.extend_from_slice(hash);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// Stores a federation of `keys` under `key_name`, in rskj's multikey
/// format, created at RSK block `creation_block`, with its format version
/// under `format_key`.
pub(super) fn store_federation<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    (key_name, format_key): (&str, &str),
    keys: &[[u8; 33]],
    creation_block: u64,
    format: u64,
) {
    let members: Vec<StoredMember> = keys.iter().copied().map(StoredMember::from_btc).collect();
    storage::bridge_store_bytes_named(
        ctx,
        key_name,
        &serialize_federation_multikey(&members, 0, creation_block),
    );
    storage::bridge_store_bytes_named(ctx, format_key, &serialization::rlp_encode_u64(format));
}

/// Where the federation that is, or is to be, active is stored.
pub(super) const NEW_FEDERATION: (&str, &str) = (
    storage::NEW_FEDERATION_KEY,
    storage::NEW_FEDERATION_FORMAT_VERSION_KEY,
);
/// Where the federation before it is stored.
pub(super) const OLD_FEDERATION: (&str, &str) = (
    storage::OLD_FEDERATION_KEY,
    storage::OLD_FEDERATION_FORMAT_VERSION_KEY,
);

/// `cfg` with every upgrade from `from` on moved to block `height`, so that
/// below it none of what they bring is active.
pub(super) fn upgrades_from_at(
    cfg: RskHardforkConfig,
    from: RskNetworkUpgrade,
    height: u64,
) -> RskHardforkConfig {
    use RskNetworkUpgrade::*;
    let moved: Vec<RskNetworkUpgrade> = [
        Orchid,
        Wasabi100,
        TwoToThree,
        Papyrus200,
        Iris300,
        Hop400,
        Fingerroot500,
        Arrowhead600,
        Arrowhead631,
        Lovell700,
        Reed800,
        Reed810,
        Vetiver900,
    ]
    .into_iter()
    .filter(|upgrade| *upgrade >= from && cfg.activation_height(*upgrade).is_some())
    .collect();
    moved
        .into_iter()
        .fold(cfg, |cfg, upgrade| cfg.with_hardfork(upgrade, Some(height)))
}

/// The Bitcoin height a peg-in in [`btc_chain_holding`] is registered at: it
/// has there the three confirmations rskj's regtest asks of one.
pub(super) const PEGIN_HEIGHT: i64 = 350;

/// Gives `ctx`'s Bridge Bitcoin regtest blocks 1 to 352, the only
/// transaction of blocks 349 and 350 being `raw`, taken under `cfg`.
pub(super) fn btc_chain_holding<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    raw: &[u8],
    cfg: &RskHardforkConfig,
) {
    let root = tx::calculate_btc_tx_hash(raw);
    let mut prev = bitcoin::constants::genesis_block(bitcoin::Network::Regtest)
        .header
        .block_hash();
    let headers: Vec<Vec<u8>> = (1..=352u32)
        .map(|height| {
            let merkle = if height == 349 || height == 350 {
                root
            } else {
                [0x11; 32]
            };
            let header = (0..)
                .map(|nonce| bitcoin::block::Header {
                    version: bitcoin::block::Version::from_consensus(1),
                    prev_blockhash: prev,
                    merkle_root: bitcoin::TxMerkleNode::from_byte_array(merkle),
                    time: 1_700_000_000,
                    bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
                    nonce,
                })
                .find(|header| btc_rules::verify_header(header, BtcNetwork::Regtest))
                .expect("a nonce under regtest's target");
            prev = header.block_hash();
            bitcoin::consensus::serialize(&header)
        })
        .collect();
    let slices: Vec<&[u8]> = headers.iter().map(Vec::as_slice).collect();
    let args = encode::args(&[Arg::BytesArray(&slices)]);
    let number = block_of(ctx);
    btc_chain::receive_headers(
        ctx,
        &args,
        0,
        &BridgeConstants::rskj_regtest(),
        cfg.has_stored_block_v2(number),
        cfg,
    )
    .expect("the headers are taken");
}

/// A partial merkle tree of one transaction, `raw`, matched: its root is the
/// transaction's hash.
pub(super) fn one_leaf_tree(raw: &[u8]) -> Vec<u8> {
    let mut tree = 1u32.to_le_bytes().to_vec();
    tree.push(1);
    tree.extend_from_slice(&tx::calculate_btc_tx_hash(raw));
    tree.extend_from_slice(&[1, 0x01]);
    tree
}

/// A transaction with one input, a P2PKH spend, paying `outputs`.
pub(super) fn tx_paying(outputs: &[(u64, bitcoin::ScriptBuf)]) -> bitcoin::Transaction {
    let mut script_sig = vec![71];
    script_sig.extend_from_slice(&[0x30; 71]);
    script_sig.push(33);
    script_sig.extend_from_slice(&public_key(&[0x77]));
    bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([0x5a; 32]),
                vout: 0,
            },
            script_sig: bitcoin::ScriptBuf::from_bytes(script_sig),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: outputs
            .iter()
            .map(|(value, script_pubkey)| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(*value),
                script_pubkey: script_pubkey.clone(),
            })
            .collect(),
    }
}

/// Gives the Bridge every RBTC there is, so that no peg-in passes the
/// locking cap for want of it.
pub(super) fn fund_bridge<CTX: crate::RskContextTr>(ctx: &mut CTX) {
    let all_rbtc = U256::from(2_100_000_000_000_000u64) * U256::from(WEI_PER_SATOSHI);
    ctx.journal_mut()
        .load_account_mut(BRIDGE_ADDR)
        .expect("the Bridge's account")
        .set_balance(all_rbtc);
}

/// How many requests the peg-out queue holds once `releaseBtc` is asked for
/// `satoshis` at `block` of `network`, with the federation of `keys` active
/// and a fee per KB of `fee_per_kb`.
pub(super) fn release_queued(
    network: (&BridgeConstants, &RskHardforkConfig),
    block: u64,
    keys: &[[u8; 33]],
    fee_per_kb: u64,
    satoshis: u64,
) -> usize {
    let mut ctx = ctx_at(block);
    store_federation(&mut ctx, NEW_FEDERATION, keys, 1, 1000);
    storage::bridge_store_u256(&mut ctx, storage::FEE_PER_KB_KEY, U256::from(fee_per_kb));
    peg::release_btc(
        &mut ctx,
        0,
        network.0,
        network.1,
        &BridgeTxContext::default(),
        RskAddress::repeat_byte(0x33),
        1,
        U256::from(satoshis) * U256::from(WEI_PER_SATOSHI),
    )
    .expect("releaseBtc answers");
    peg::load_release_request_queue(&mut ctx, true).len()
}

/// A majority of the network's federation-change authorizers, as many as
/// decide a vote (`FederationConstants.federationChangeAuthorizer`).
pub(super) fn deciding_authorizers(config: &BridgeConstants) -> Vec<RskAddress> {
    let all: Vec<RskAddress> = config
        .federation_change_authorizer_keys
        .iter()
        .map(|key| {
            let key = hex::decode(key).expect("a key in hex");
            federation::rsk_address_from_public_key(&key).expect("a public key")
        })
        .collect();
    all[..all.len() / 2 + 1].to_vec()
}

/// The context of a transaction `sender` sends.
pub(super) fn sent_by(sender: RskAddress) -> BridgeTxContext {
    BridgeTxContext {
        rsk_sender: sender,
        ..BridgeTxContext::default()
    }
}

/// What a Bridge method answers, where it does not throw.
fn answer(
    out: Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError>,
    method: &str,
) -> i128 {
    out.map(|out| int_answer(&out.bytes))
        .unwrap_or_else(|e| panic!("{method} throws: {e:?}"))
}

/// Votes, as a majority of the network's federation-change authorizers, at
/// the block of `ctx`, to create a pending federation and add `members` to it
/// (each key as its member's BTC, RSK and MST key). Each vote answers 1.
pub(super) fn pend_federation<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    network: (&BridgeConstants, &RskHardforkConfig),
    members: &[[u8; 33]],
) {
    let (config, cfg) = network;
    let deciding = deciding_authorizers(config);
    for &sender in &deciding {
        let out = governance::create_federation(ctx, 0, config, cfg, &sent_by(sender));
        assert_eq!(answer(out, "createFederation"), 1, "createFederation");
    }
    for member in members {
        let args = encode::args(&[Arg::Bytes(member), Arg::Bytes(member), Arg::Bytes(member)]);
        for &sender in &deciding {
            let out = governance::add_federator_public_key_multikey(
                ctx,
                &args,
                0,
                config,
                cfg,
                &sent_by(sender),
            );
            assert_eq!(
                answer(out, "addFederatorPublicKeyMultikey"),
                1,
                "addFederatorPublicKeyMultikey"
            );
        }
    }
}

/// [`pend_federation`], then the votes to commit it with the hash
/// `getPendingFederationHash` answers. Each vote before the commit's deciding
/// one answers 1; the commit's answer is `None` where its deciding vote
/// throws.
pub(super) fn elect_federation<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    network: (&BridgeConstants, &RskHardforkConfig),
    members: &[[u8; 33]],
) -> Option<i128> {
    pend_federation(ctx, network, members);
    let (config, cfg) = network;
    let hash =
        getters::get_pending_federation_hash(ctx, 0).expect("getPendingFederationHash answers");
    let length = U256::from_be_slice(&hash.bytes[32..64]).to::<usize>();
    let args = encode::args(&[Arg::Bytes(&hash.bytes[64..64 + length])]);
    let deciding = deciding_authorizers(config);
    let (deciding_vote, before) = deciding.split_last().expect("authorizers");
    for &sender in before {
        let out = governance::commit_federation(ctx, &args, 0, config, cfg, &sent_by(sender));
        assert_eq!(answer(out, "commitFederation"), 1, "commitFederation");
    }
    governance::commit_federation(ctx, &args, 0, config, cfg, &sent_by(*deciding_vote))
        .ok()
        .map(|out| int_answer(&out.bytes))
}

/// An `int256` result, which here always fits an `i128`.
pub(super) fn int_answer(out: &[u8]) -> i128 {
    assert_eq!(out.len(), 32, "an int256 result");
    let (high, low) = out.split_at(16);
    let low = i128::from_be_bytes(low.try_into().expect("16 bytes"));
    let fill = if low < 0 { 0xff } else { 0x00 };
    assert!(high.iter().all(|&b| b == fill), "an int256 past an i128");
    low
}

/// A call of `registerFastBridgeBtcTransaction`, as the contract `lbc` makes it.
pub(super) struct FlyoverCall<'a> {
    pub(super) tx: &'a [u8],
    pub(super) height: i64,
    pub(super) derivation_arguments: [u8; 32],
    pub(super) user: &'a [u8],
    pub(super) lbc: RskAddress,
    pub(super) lp: &'a [u8],
}

impl FlyoverCall<'_> {
    /// rskj `PegUtils.getFlyoverDerivationHash`, over the addresses as
    /// `BridgeUtils.serializeBtcAddressWithVersion` writes them.
    pub(super) fn derivation_hash(&self, user: &[u8], lp: &[u8]) -> [u8; 32] {
        let mut preimage = self.derivation_arguments.to_vec();
        preimage.extend_from_slice(user);
        preimage.extend_from_slice(self.lbc.as_slice());
        preimage.extend_from_slice(lp);
        sha3::Keccak256::digest(&preimage).into()
    }

    /// What the method answers, its liquidity provider's flag false.
    pub(super) fn answer<CTX: crate::RskContextTr>(
        &self,
        ctx: &mut CTX,
        config: &BridgeConstants,
        cfg: &RskHardforkConfig,
    ) -> i128 {
        let tree = one_leaf_tree(self.tx);
        let mut lbc_word = [0u8; 32];
        lbc_word[12..].copy_from_slice(self.lbc.as_slice());
        let args = encode::args(&[
            Arg::Bytes(self.tx),
            Arg::Word(encode::int(self.height)),
            Arg::Bytes(&tree),
            Arg::Word(self.derivation_arguments),
            Arg::Bytes(self.user),
            Arg::Word(lbc_word),
            Arg::Bytes(self.lp),
            Arg::Word([0; 32]),
        ]);
        let out = peg::register_fast_bridge_btc_transaction(
            ctx,
            &args,
            0,
            config,
            cfg,
            &BridgeTxContext::default(),
            self.lbc,
            2,
        )
        .expect("the method answers");
        int_answer(&out.bytes)
    }
}

/// rskj `FlyoverRedeemScriptBuilderImpl.of`: `<hash> OP_DROP` before the
/// federation's redeem script, and its P2SH output.
pub(super) fn flyover_output(
    derivation_hash: &[u8; 32],
    federation_redeem: &[u8],
) -> bitcoin::ScriptBuf {
    let mut script = vec![32];
    script.extend_from_slice(derivation_hash);
    script.push(0x75);
    script.extend_from_slice(federation_redeem);
    p2sh(&hash160(&script))
}

/// Marks a Flyover derivation hash used for the Bitcoin transaction `raw`,
/// as rskj's `BridgeStorageProvider.markFlyoverDerivationHashAsUsed` does.
pub(super) fn mark_flyover_hash_used<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    raw: &[u8],
    derivation_hash: &[u8; 32],
) {
    let mut display = tx::calculate_btc_tx_hash(raw);
    display.reverse();
    let identifier = format!("{}{}", hex::encode(display), hex::encode(derivation_hash));
    let key = storage::compound_key(storage::FAST_BRIDGE_HASH_USED_KEY, "-", &identifier);
    storage::bridge_store_raw(ctx, key, Some(vec![1]));
}
