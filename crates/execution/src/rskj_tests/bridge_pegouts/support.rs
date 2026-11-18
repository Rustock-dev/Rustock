//! What this folder's ports share: the Bridge at a block of a network, the
//! calls rskj's tests make to it, and the cells of its storage they read.
//!
//! rskj's tests build a `BridgeStorageProvider` or a `BridgeSupport` over a
//! mocked repository, with an `ActivationConfig` that has the rules a test
//! names. Here the Bridge runs in a context with no trie, at a block of
//! mainnet's own schedule or of a ladder that ends at an upgrade ([`up_to`]),
//! so that a rule is on or off as a network has it. What rskj writes and
//! rustock is held to (a serialized queue, a cell's bytes, a redeem script) is
//! written here from rskj's definition, not with rustock's own serializers.

use alloy_primitives::{Address, U256};
use revm::context_interface::{ContextTr, JournalTr};
use revm::precompile::{PrecompileError, PrecompileOutput};

pub(super) use crate::bridge::constants::BridgeConstants;
pub(super) use crate::bridge::serialization::{rlp_encode_element, rlp_encode_list};
pub(super) use crate::bridge::storage::rlp_encode_uint;
pub(super) use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade, RSK_REGTEST_CHAIN_ID};
pub(super) use crate::precompiles::BridgeTxContext;

/// The world a ported test runs the Bridge in: no trie and no database, only
/// what the test and the Bridge write.
pub(super) type Ctx = revm::Context<
    revm::context::BlockEnv,
    revm::context::TxEnv,
    revm::context::CfgEnv,
    revm::database::EmptyDB,
    revm::Journal<revm::database::EmptyDB>,
    crate::raw_storage::RskChainExt,
>;

/// Mainnet's papyrus200, which brings RSKIP134, RSKIP143 and RSKIP146.
pub(super) const MAINNET_PAPYRUS200: u64 = 2_392_700;
/// Mainnet's iris300, which brings RSKIP176, RSKIP185, RSKIP199 and RSKIP200.
pub(super) const MAINNET_IRIS300: u64 = 3_614_800;
/// Mainnet's hop400, which brings RSKIP271 and RSKIP293.
pub(super) const MAINNET_HOP400: u64 = 4_598_500;
/// Mainnet's fingerroot500, which brings RSKIP326.
pub(super) const MAINNET_FINGERROOT500: u64 = 5_468_000;
/// Mainnet's arrowhead600, which brings RSKIP379.
pub(super) const MAINNET_ARROWHEAD600: u64 = 6_223_700;
/// Mainnet's lovell700, which brings RSKIP419, RSKIP427, RSKIP428 and RSKIP459.
pub(super) const MAINNET_LOVELL700: u64 = 7_338_024;
/// Mainnet's reed800, which brings RSKIP305.
pub(super) const MAINNET_REED800: u64 = 8_052_200;
/// Mainnet's vetiver900, which brings RSKIP551.
pub(super) const MAINNET_VETIVER900: u64 = 8_804_200;
/// Testnet's lovell700.
pub(super) const TESTNET_LOVELL700: u64 = 6_110_487;

/// One RBTC satoshi in wei.
pub(super) const WEI_PER_SATOSHI: u64 = 10_000_000_000;

/// The upgrades after genesis, in order.
const LADDER: [RskNetworkUpgrade; 13] = [
    RskNetworkUpgrade::Orchid,
    RskNetworkUpgrade::Wasabi100,
    RskNetworkUpgrade::TwoToThree,
    RskNetworkUpgrade::Papyrus200,
    RskNetworkUpgrade::Iris300,
    RskNetworkUpgrade::Hop400,
    RskNetworkUpgrade::Fingerroot500,
    RskNetworkUpgrade::Arrowhead600,
    RskNetworkUpgrade::Arrowhead631,
    RskNetworkUpgrade::Lovell700,
    RskNetworkUpgrade::Reed800,
    RskNetworkUpgrade::Reed810,
    RskNetworkUpgrade::Vetiver900,
];

/// rskj's `ActivationConfigsForTest.<upgrade>()`: every upgrade up to `last`
/// active from block 0 and none after it, on the network `chain_id` names.
/// `Genesis` is rskj's `genesis()`, with nothing active.
pub(super) fn up_to(last: RskNetworkUpgrade, chain_id: u64) -> RskHardforkConfig {
    LADDER
        .into_iter()
        .fold(RskHardforkConfig::all_active(chain_id), |cfg, upgrade| {
            cfg.with_hardfork(upgrade, (upgrade <= last).then_some(0))
        })
}

/// A context at block `number`, with nothing stored.
pub(super) fn ctx_at(number: u64) -> Ctx {
    use revm::MainContext;
    let mut ctx: Ctx =
        revm::Context::mainnet().with_chain(crate::raw_storage::RskChainExt::default());
    ctx.block.number = U256::from(number);
    ctx.block.timestamp = U256::from(1_700_000_000u64);
    ctx
}

/// What the Bridge's cell `key` holds, `None` where it holds nothing.
pub(super) fn cell(ctx: &mut Ctx, key: U256) -> Option<Vec<u8>> {
    crate::bridge::storage::bridge_load_raw(ctx, key)
}

/// The cell rskj keys `DataWord.fromString(name)`.
pub(super) fn named(ctx: &mut Ctx, name: &str) -> Option<Vec<u8>> {
    cell(ctx, crate::bridge::storage::bridge_storage_key(name))
}

/// Puts `value` in the cell rskj keys `DataWord.fromString(name)`.
pub(super) fn put_named(ctx: &mut Ctx, name: &str, value: &[u8]) {
    crate::bridge::storage::bridge_store_raw(
        ctx,
        crate::bridge::storage::bridge_storage_key(name),
        Some(value.to_vec()),
    );
}

/// The cell rskj keys `<base>-<identifier>` (`getCompoundKey("-", ..)`),
/// which is `DataWord.fromLongString`: the Keccak-256 of the string.
pub(super) fn compound(base: &str, identifier: &str) -> U256 {
    use sha3::Digest;
    let hash: [u8; 32] = sha3::Keccak256::digest(format!("{base}-{identifier}")).into();
    U256::from_be_bytes(hash)
}

/// rskj `serializeLong`: `RLP.encodeBigInteger(BigInteger.valueOf(value))`.
pub(super) fn serialize_long(value: u64) -> Vec<u8> {
    rlp_encode_uint(U256::from(value))
}

/// RIPEMD-160 of SHA-256, Bitcoin's hash of a key or a script.
pub(super) fn hash160(data: &[u8]) -> [u8; 20] {
    use ripemd::Digest as _;
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}

/// bitcoinj-thin's `ScriptBuilder.createRedeemScript(threshold, keys)`: the
/// keys sorted, and the counts as its `ScriptBuilder.number` writes them, OP_1
/// to OP_15 below 16 and a one-byte push from 16 on.
pub(super) fn multisig(keys: &[[u8; 33]], threshold: usize) -> Vec<u8> {
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

/// A network's genesis federation's keys (`genesisFederationPublicKeys`).
pub(super) fn genesis_keys(constants: &BridgeConstants) -> Vec<[u8; 33]> {
    constants
        .genesis_federation_public_keys
        .iter()
        .map(|key| {
            alloy_primitives::hex::decode(key)
                .expect("a key in hex")
                .try_into()
                .expect("a compressed key")
        })
        .collect()
}

/// rskj `StandardMultiSigFederation.getRedeemScript`: `size / 2 + 1` of its
/// members.
pub(super) fn standard_redeem(keys: &[[u8; 33]]) -> Vec<u8> {
    multisig(keys, keys.len() / 2 + 1)
}

/// `OP_HASH160 <hash> OP_EQUAL`.
pub(super) fn p2sh(hash: &[u8; 20]) -> Vec<u8> {
    let mut script = vec![0xa9, 20];
    script.extend_from_slice(hash);
    script.push(0x87);
    script
}

/// `OP_DUP OP_HASH160 <hash> OP_EQUALVERIFY OP_CHECKSIG`.
pub(super) fn p2pkh(hash: &[u8; 20]) -> Vec<u8> {
    let mut script = vec![0x76, 0xa9, 20];
    script.extend_from_slice(hash);
    script.extend_from_slice(&[0x88, 0xac]);
    script
}

/// Gives the Bridge's active federation, the genesis federation of
/// `constants`, one UTXO of `satoshis` paid to its P2SH script, in the cell
/// `cfg` keeps them in at `block`.
pub(super) fn fund_genesis_federation(
    ctx: &mut Ctx,
    constants: &BridgeConstants,
    cfg: &RskHardforkConfig,
    block: u64,
    satoshis: u64,
) {
    let script = p2sh(&hash160(&standard_redeem(&genesis_keys(constants))));
    let utxo = crate::bridge::storage::BridgeUtxo {
        tx_hash: [0x3c; 32],
        vout: 0,
        value_satoshis: satoshis,
        height: 1,
        script,
        coinbase: false,
    };
    crate::bridge::storage::store_federation_utxos(ctx, cfg, block, &[utxo]);
}

/// A release request as rskj's `serializeReleaseRequestQueue` writes it: the
/// destination's hash160 and the amount, without the transaction's hash.
pub(super) fn legacy_queue(entries: &[([u8; 20], u64)]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = entries
        .iter()
        .flat_map(|(destination, satoshis)| {
            [rlp_encode_element(destination), serialize_long(*satoshis)]
        })
        .collect();
    rlp_encode_list(&items)
}

/// Release requests as rskj's `serializeReleaseRequestQueueWithTxHash` writes
/// them: the destination's hash160, the amount and the transaction's hash.
pub(super) fn hashed_queue(entries: &[([u8; 20], u64, [u8; 32])]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = entries
        .iter()
        .flat_map(|(destination, satoshis, hash)| {
            [
                rlp_encode_element(destination),
                serialize_long(*satoshis),
                rlp_encode_element(hash),
            ]
        })
        .collect();
    rlp_encode_list(&items)
}

/// An `int256` answer, which here always fits an `i128`.
pub(super) fn int256(answer: &[u8]) -> i128 {
    assert_eq!(answer.len(), 32, "an int256 answer is one word");
    let (high, low) = answer.split_at(16);
    let low = i128::from_be_bytes(low.try_into().expect("16 bytes"));
    let fill = if low < 0 { 0xff } else { 0x00 };
    assert!(high.iter().all(|&b| b == fill), "an int256 past an i128");
    low
}

/// The compressed public key of the private key `private`, read as rskj's
/// `BtcECKey.fromPrivate` reads it: a big-endian number.
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

/// Where the active federation is stored, and its format.
pub(super) const NEW_FEDERATION: (&str, &str) = ("newFederation", "newFederationFormatVersion");
/// Where the federation before it is stored, and its format.
pub(super) const OLD_FEDERATION: (&str, &str) = ("oldFederation", "oldFederationFormatVersion");

/// Stores a federation of `keys` where `slot` names, in rskj's multikey
/// format, made at RSK block `creation_block`, with its format version.
pub(super) fn store_federation(
    ctx: &mut Ctx,
    slot: (&str, &str),
    keys: &[[u8; 33]],
    creation_block: u64,
    format: u64,
) {
    use crate::bridge::federation::StoredMember;
    let members: Vec<StoredMember> = keys.iter().copied().map(StoredMember::from_btc).collect();
    store_members(ctx, slot, &members, creation_block, format);
}

/// Stores a federation of `members`, each with its own RSK and MST keys, as
/// [`store_federation`] stores one of BTC keys alone.
pub(super) fn store_members(
    ctx: &mut Ctx,
    (key_name, format_key): (&str, &str),
    members: &[crate::bridge::federation::StoredMember],
    creation_block: u64,
    format: u64,
) {
    use crate::bridge::federation::serialize_federation_multikey;
    put_named(
        ctx,
        key_name,
        &serialize_federation_multikey(members, 0, creation_block),
    );
    put_named(ctx, format_key, &serialize_long(format));
}

/// Gives the Bridge every RBTC there is, so that no peg-in passes the
/// locking cap for want of it.
pub(super) fn fund_bridge(ctx: &mut Ctx) {
    use revm::context_interface::journaled_state::account::JournaledAccountTr;
    let all_rbtc = U256::from(2_100_000_000_000_000u64) * U256::from(WEI_PER_SATOSHI);
    ctx.journal_mut()
        .load_account_mut(crate::precompiles::BRIDGE_ADDR)
        .expect("the Bridge's account")
        .set_balance(all_rbtc);
}

/// bitcoinj `Sha256Hash.twiceOf`: SHA-256 of SHA-256, in the order computed,
/// which is a Bitcoin hash's internal order.
pub(super) fn sha256d(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(sha2::Sha256::digest(data)).into()
}

/// A hash the other way round: a Bitcoin hash's display order, which is
/// bitcoinj's `Sha256Hash` of a transaction or a block.
pub(super) fn reversed(mut hash: [u8; 32]) -> [u8; 32] {
    hash.reverse();
    hash
}

/// A partial merkle tree of one transaction, its hash `hash` matched: its
/// root is the hash.
pub(super) fn one_leaf_tree(hash: &[u8; 32]) -> Vec<u8> {
    let mut tree = 1u32.to_le_bytes().to_vec();
    tree.push(1);
    tree.extend_from_slice(hash);
    tree.extend_from_slice(&[1, 0x01]);
    tree
}

/// A regtest header on `prev` whose merkle root is `merkle_root`, mined to
/// regtest's easiest target.
pub(super) fn mine(prev: bitcoin::BlockHash, merkle_root: [u8; 32]) -> bitcoin::block::Header {
    use bitcoin::hashes::Hash;
    (0..)
        .map(|nonce| bitcoin::block::Header {
            version: bitcoin::block::Version::from_consensus(1),
            prev_blockhash: prev,
            merkle_root: bitcoin::TxMerkleNode::from_byte_array(merkle_root),
            time: 1_700_000_000,
            bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
            nonce,
        })
        .find(|header| {
            crate::bridge::btc_rules::verify_header(
                header,
                crate::bridge::constants::BtcNetwork::Regtest,
            )
        })
        .expect("a nonce under regtest's target")
}

/// Bitcoin regtest headers 1 to `length`, each on the one before from the
/// genesis block, block `height`'s merkle root `root(height)`.
pub(super) fn regtest_headers(
    length: u32,
    root: impl Fn(u32) -> [u8; 32],
) -> Vec<bitcoin::block::Header> {
    let mut prev = bitcoin::constants::genesis_block(bitcoin::Network::Regtest)
        .header
        .block_hash();
    (1..=length)
        .map(|height| {
            let header = mine(prev, root(height));
            prev = header.block_hash();
            header
        })
        .collect()
}

/// A header's hash as bitcoinj's `Sha256Hash` of it reads: display order.
pub(super) fn block_hash(header: &bitcoin::block::Header) -> [u8; 32] {
    use bitcoin::hashes::Hash;
    reversed(header.block_hash().to_byte_array())
}

/// rskj `receiveHeaders` of `headers`, on rskj regtest's constants.
pub(super) fn receive_headers(
    ctx: &mut Ctx,
    headers: &[bitcoin::block::Header],
    cfg: &RskHardforkConfig,
) {
    use crate::abi::encode::{self, Arg};
    let serialized: Vec<Vec<u8>> = headers.iter().map(bitcoin::consensus::serialize).collect();
    let slices: Vec<&[u8]> = serialized.iter().map(Vec::as_slice).collect();
    let number = revm::context_interface::Block::number(ctx.block()).to::<u64>();
    crate::bridge::btc_chain::receive_headers(
        ctx,
        &encode::args(&[Arg::BytesArray(&slices)]),
        0,
        &BridgeConstants::rskj_regtest(),
        cfg.has_stored_block_v2(number),
        cfg,
    )
    .expect("the headers are taken");
}

/// The Bitcoin height a peg-in is registered at: with blocks to
/// [`BTC_HEAD`] it has the three confirmations rskj's regtest asks of one.
pub(super) const PEGIN_HEIGHT: u32 = 350;
/// The head of the Bitcoin chain [`btc_chain`] stores.
pub(super) const BTC_HEAD: u32 = 352;

/// Bitcoin regtest blocks 1 to [`BTC_HEAD`] in the Bridge's store, received
/// under `cfg`, block `height`'s merkle root `root(height)`. The headers.
pub(super) fn btc_chain(
    ctx: &mut Ctx,
    cfg: &RskHardforkConfig,
    root: impl Fn(u32) -> [u8; 32],
) -> Vec<bitcoin::block::Header> {
    let headers = regtest_headers(BTC_HEAD, root);
    receive_headers(ctx, &headers, cfg);
    headers
}

/// A transaction of one input, a P2PKH spend by the key of the private key
/// `0x77`, paying `outputs`.
pub(super) fn tx_paying(outputs: &[(u64, Vec<u8>)]) -> bitcoin::Transaction {
    use bitcoin::hashes::Hash;
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
            .map(|(value, script)| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(*value),
                script_pubkey: bitcoin::ScriptBuf::from_bytes(script.clone()),
            })
            .collect(),
    }
}

/// rskj `registerBtcTransaction(raw, height, tree)` on rskj regtest's
/// constants, by a transaction of `tx`'s.
pub(super) fn register(
    ctx: &mut Ctx,
    cfg: &RskHardforkConfig,
    tx: &BridgeTxContext,
    raw: &[u8],
    height: u32,
    tree: &[u8],
) -> Result<PrecompileOutput, PrecompileError> {
    use crate::abi::encode::{self, Arg};
    let args = encode::args(&[
        Arg::Bytes(raw),
        Arg::Word(encode::int(i64::from(height))),
        Arg::Bytes(tree),
    ]);
    crate::bridge::peg::register_btc_transaction(
        ctx,
        &args,
        0,
        &BridgeConstants::rskj_regtest(),
        cfg,
        tx,
    )
}

/// A transaction's txid, without its witnesses, in the order its bytes are
/// computed (bitcoinj's `getHash(false)` is the other way round).
pub(super) fn txid(tx: &bitcoin::Transaction) -> [u8; 32] {
    use bitcoin::hashes::Hash;
    tx.compute_txid().to_byte_array()
}

/// `data` pushed as bitcoinj's `ScriptBuilder.data` pushes it: no bytes as
/// OP_0, up to 75 directly, then OP_PUSHDATA1 and OP_PUSHDATA2.
pub(super) fn push(data: &[u8]) -> Vec<u8> {
    let mut out = match data.len() {
        0 => return vec![0x00],
        n @ 1..=75 => vec![n as u8],
        n @ 76..=255 => vec![0x4c, n as u8],
        n => [&[0x4d][..], &(n as u16).to_le_bytes()].concat(),
    };
    out.extend_from_slice(data);
    out
}

/// A P2PKH spend's scriptSig: a signature's worth of bytes and the key of
/// the private key `0x77`.
pub(super) fn p2pkh_spend() -> Vec<u8> {
    [push(&[0x30; 71]), push(&public_key(&[0x77]))].concat()
}

/// rskj `ScriptBuilder.createP2SHMultiSigInputScript` of `redeem` with its
/// signatures in: OP_0, a signature's worth of bytes for each the script asks
/// for, and the script pushed last.
pub(super) fn multisig_spend(redeem: &[u8]) -> Vec<u8> {
    let threshold = usize::from(redeem[0] - 0x50);
    let mut script = vec![0x00];
    for _ in 0..threshold {
        script.extend(push(&[0x30; 71]));
    }
    script.extend(push(redeem));
    script
}

/// The address a federation of `keys` is paid at: the P2SH of its redeem
/// script.
pub(super) fn address_of(keys: &[[u8; 33]]) -> Vec<u8> {
    p2sh(&hash160(&standard_redeem(keys)))
}

/// A P2WPKH output script, which rskj's tests write as a bech32 address.
pub(super) fn p2wpkh(hash: &[u8; 20]) -> Vec<u8> {
    [&[0x00, 20][..], hash].concat()
}

/// An input spending output 0 of the transaction `[tag; 32]`, with
/// `script_sig`.
pub(super) fn input(tag: u8, script_sig: Vec<u8>) -> bitcoin::TxIn {
    use bitcoin::hashes::Hash;
    bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([tag; 32]),
            vout: 0,
        },
        script_sig: bitcoin::ScriptBuf::from_bytes(script_sig),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    }
}

/// A transaction of version 1, as rskj's `new BtcTransaction(params)` makes
/// one, of `inputs`, paying `outputs`.
pub(super) fn btc_transaction(
    inputs: Vec<bitcoin::TxIn>,
    outputs: &[(u64, Vec<u8>)],
) -> bitcoin::Transaction {
    bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: inputs,
        output: outputs
            .iter()
            .map(|(value, script)| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(*value),
                script_pubkey: bitcoin::ScriptBuf::from_bytes(script.clone()),
            })
            .collect(),
    }
}

/// rskj `BitcoinUtils.getSigHashForPegoutIndex` of a transaction whose
/// inputs carry no signatures: `hashForSignature(0, redeem, ALL, false)`,
/// every scriptSig cleared and the first input's set to `redeem`, the
/// witnesses kept, SIGHASH_ALL in four bytes after it, hashed twice.
pub(super) fn index_sighash(tx: &bitcoin::Transaction, redeem: &[u8]) -> [u8; 32] {
    let mut copy = tx.clone();
    for (index, input) in copy.input.iter_mut().enumerate() {
        input.script_sig = if index == 0 {
            bitcoin::ScriptBuf::from_bytes(redeem.to_vec())
        } else {
            bitcoin::ScriptBuf::new()
        };
    }
    let mut preimage = bitcoin::consensus::serialize(&copy);
    preimage.extend_from_slice(&1u32.to_le_bytes());
    sha256d(&preimage)
}

/// rskj `setPegoutTxSigHash`: `sighash` in the pegout index, the one true
/// byte under `pegoutTxSigHash-<sighash>`.
pub(super) fn index_pegout(ctx: &mut Ctx, sighash: &[u8; 32]) {
    crate::bridge::storage::bridge_store_raw(
        ctx,
        compound("pegoutTxSigHash", &alloy_primitives::hex::encode(sighash)),
        Some(vec![1]),
    );
}

/// What the Bridge logged, as the ports read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    /// `pegin_btc`, its amount and its protocol version.
    PeginBtc(u64, u64),
    /// `rejected_pegin`, its `RejectedPeginReason`.
    Rejected(u64),
    /// `unrefundable_pegin`, its `NonRefundablePeginReason`.
    Unrefundable(u64),
    /// `release_requested`, its amount.
    ReleaseRequested(u64),
    /// `pegout_transaction_created`.
    PegoutTransactionCreated,
    /// Any other, by its first topic.
    Other([u8; 32]),
}

/// The Keccak-256 of a Solidity event's signature, its first topic.
fn topic(signature: &str) -> [u8; 32] {
    use sha3::Digest;
    sha3::Keccak256::digest(signature).into()
}

/// The Bridge's events among `logs`.
pub(super) fn events_of(logs: &[revm::primitives::Log]) -> Vec<Event> {
    let word = |data: &[u8]| U256::from_be_slice(&data[..32]).to::<u64>();
    logs.iter()
        .map(|log| {
            let first: [u8; 32] = log.data.topics()[0].0;
            let data = &log.data.data;
            if first == topic("pegin_btc(address,bytes32,int256,int256)") {
                Event::PeginBtc(word(data), word(&data[32..]))
            } else if first == topic("rejected_pegin(bytes32,int256)") {
                Event::Rejected(word(data))
            } else if first == topic("unrefundable_pegin(bytes32,int256)") {
                Event::Unrefundable(word(data))
            } else if first == topic("release_requested(bytes32,bytes32,uint256)") {
                Event::ReleaseRequested(word(data))
            } else if first == topic("pegout_transaction_created(bytes32,bytes)") {
                Event::PegoutTransactionCreated
            } else {
                Event::Other(first)
            }
        })
        .collect()
}

/// What `registerBtcTransaction` left.
#[derive(Debug)]
pub(super) struct Outcome {
    /// Whether the call returned; a call that throws leaves nothing.
    pub(super) returned: bool,
    /// The RSK block the transaction was marked processed at.
    pub(super) processed_at: Option<u64>,
    pub(super) events: Vec<Event>,
    /// The values of the active federation's UTXOs.
    pub(super) active_utxos: Vec<u64>,
    /// The values of the retiring federation's UTXOs.
    pub(super) retiring_utxos: Vec<u64>,
    /// The peg-outs, refunds among them, waiting for confirmations.
    pub(super) refunds: usize,
}

/// A network's Bridge at an RSK block, for rskj's `registerBtcTransaction`:
/// the Bridge holding all the RBTC there is, and the lock whitelist off, as
/// mainnet's and testnet's have long been.
pub(super) struct Registration {
    pub(super) ctx: Ctx,
    pub(super) cfg: RskHardforkConfig,
    pub(super) constants: BridgeConstants,
    pub(super) block: u64,
}

impl Registration {
    pub(super) fn new(cfg: RskHardforkConfig, constants: BridgeConstants, block: u64) -> Self {
        let mut ctx = ctx_at(block);
        fund_bridge(&mut ctx);
        crate::bridge::storage::store_one_off_whitelist(&mut ctx, &[], 0);
        Self {
            ctx,
            cfg,
            constants,
            block,
        }
    }

    /// Mainnet's Bridge at `block`.
    pub(super) fn mainnet(block: u64) -> Self {
        Self::new(
            RskHardforkConfig::mainnet(),
            BridgeConstants::mainnet(),
            block,
        )
    }

    /// The federation of `keys` where `slot` names, made at RSK block 1.
    pub(super) fn federation(&mut self, slot: (&str, &str), keys: &[[u8; 33]]) {
        store_federation(&mut self.ctx, slot, keys, 1, 1000);
    }

    /// rskj's `mockChainOfStoredBlocks`: Bitcoin blocks stored from `height`
    /// up, the one at `height` with `merkle_root` and the network's
    /// confirmations above it, the last the chain's head. The hash of the one
    /// at `height`, as bitcoinj's `Sha256Hash` of it reads.
    pub(super) fn store_chain(&mut self, height: u32, merkle_root: [u8; 32]) -> [u8; 32] {
        use crate::bridge::btc_store::{put_stored_block, store_chain_head, StoredBlock};
        use bitcoin::hashes::Hash;
        let use_v2 = self.cfg.has_stored_block_v2(self.block);
        let head = height + self.constants.btc2rsk_minimum_acceptable_confirmations;
        let mut prev = bitcoin::BlockHash::from_byte_array([2; 32]);
        let mut at_height = [0; 32];
        for at in height..=head {
            let root = if at == height {
                merkle_root
            } else {
                [0x11; 32]
            };
            let header = bitcoin::block::Header {
                version: bitcoin::block::Version::from_consensus(1),
                prev_blockhash: prev,
                merkle_root: bitcoin::TxMerkleNode::from_byte_array(root),
                time: 1,
                bits: bitcoin::CompactTarget::from_consensus(1),
                nonce: at,
            };
            let stored = StoredBlock::new(header, at, U256::from(at));
            put_stored_block(&mut self.ctx, &stored, use_v2);
            if at == head {
                store_chain_head(&mut self.ctx, &stored, use_v2);
            }
            if at == height {
                at_height = block_hash(&header);
            }
            prev = header.block_hash();
        }
        at_height
    }

    /// rskj `registerBtcTransaction` of `tx`, in the only block of its own at
    /// Bitcoin height `height`, by an RSK transaction of its own. A transaction
    /// with witnesses is proven, as rskj proves one, by the root of its block's
    /// witnesses that `registerBtcCoinbaseTransaction` keeps. The call's
    /// writes are kept when it returns and dropped when it fails.
    pub(super) fn register(&mut self, tx: &bitcoin::Transaction, height: u32) -> Outcome {
        use crate::abi::encode::{self, Arg};
        let raw = bitcoin::consensus::serialize(tx);
        let hash = sha256d(&raw);
        let txid = txid(tx);
        let block = self.store_chain(height, txid);
        if hash != txid {
            crate::bridge::storage::bridge_store_raw(
                &mut self.ctx,
                compound("coinbaseInformation", &alloy_primitives::hex::encode(block)),
                Some(rlp_encode_list(&[rlp_encode_element(&reversed(hash))])),
            );
        }
        let tree = one_leaf_tree(&hash);
        let args = encode::args(&[
            Arg::Bytes(&raw),
            Arg::Word(encode::int(i64::from(height))),
            Arg::Bytes(&tree),
        ]);
        let tx_ctx = BridgeTxContext {
            rsk_tx_hash: [0x7e; 32],
            origin_tx_hash: [0x7e; 32],
            ..Default::default()
        };
        self.ctx.chain_mut().raw_storage.commit_call();
        let checkpoint = self.ctx.journal_mut().checkpoint();
        let returned = crate::bridge::peg::register_btc_transaction(
            &mut self.ctx,
            &args,
            0,
            &self.constants,
            &self.cfg,
            &tx_ctx,
        )
        .is_ok();
        if returned {
            self.ctx.chain_mut().raw_storage.commit_call();
            self.ctx.journal_mut().checkpoint_commit();
        } else {
            self.ctx.chain_mut().raw_storage.discard_call();
            self.ctx.journal_mut().checkpoint_revert(checkpoint);
        }
        let events = events_of(&self.ctx.journal_mut().take_logs());
        let values = |utxos: Vec<crate::bridge::storage::BridgeUtxo>| {
            utxos
                .iter()
                .map(|utxo| utxo.value_satoshis)
                .collect::<Vec<_>>()
        };
        let active =
            crate::bridge::storage::load_federation_utxos(&mut self.ctx, &self.cfg, self.block);
        let retiring = crate::bridge::storage::load_old_federation_utxos(&mut self.ctx);
        let rskip146 = self.cfg.has_rskip146(self.block);
        Outcome {
            returned,
            processed_at: crate::bridge::tx::get_btc_tx_processed_height(&mut self.ctx, &txid),
            events,
            active_utxos: values(active),
            retiring_utxos: values(retiring),
            refunds: crate::bridge::peg::load_pegout_confirmation_set(&mut self.ctx, rskip146)
                .len(),
        }
    }
}

/// Mainnet's `btcHeightWhenPegoutTxIndexActivates` and its
/// `pegoutTxIndexGracePeriodInBtcBlocks`: from RSKIP379 a transaction in a
/// Bitcoin block this high or higher is classified by the pegout index.
pub(super) const INDEX_START: u32 = 837_589 + 4_320;
/// The Bitcoin block below it, classified by the legacy rules.
pub(super) const BELOW_INDEX: u32 = INDEX_START - 1;

/// One of the runs rskj's registration tests make: a block of mainnet, and
/// the Bitcoin height the transaction is registered at.
#[derive(Clone, Copy, Debug)]
pub(super) struct Run {
    pub(super) block: u64,
    pub(super) height: u32,
}

/// rskj's `fingerrootActivations`, before RSKIP379.
pub(super) const BEFORE_RSKIP379: Run = Run {
    block: MAINNET_FINGERROOT500,
    height: BELOW_INDEX,
};
/// rskj's `arrowhead600Activations` with `shouldUsePegoutTxIndex` false.
pub(super) const RSKIP379_BELOW_INDEX: Run = Run {
    block: MAINNET_ARROWHEAD600,
    height: BELOW_INDEX,
};
/// rskj's `arrowhead600Activations` with `shouldUsePegoutTxIndex` true.
pub(super) const RSKIP379_INDEXED: Run = Run {
    block: MAINNET_ARROWHEAD600,
    height: INDEX_START,
};
/// lovell700, which rskj's runs leave out, by the index: RSKIP459.
pub(super) const RSKIP459_INDEXED: Run = Run {
    block: MAINNET_LOVELL700,
    height: INDEX_START,
};
/// rskj's `allActivations`, by the index: RSKIP551.
pub(super) const RSKIP551_INDEXED: Run = Run {
    block: MAINNET_VETIVER900,
    height: INDEX_START,
};

pub(super) const RUNS: [Run; 5] = [
    BEFORE_RSKIP379,
    RSKIP379_BELOW_INDEX,
    RSKIP379_INDEXED,
    RSKIP459_INDEXED,
    RSKIP551_INDEXED,
];

impl Run {
    pub(super) fn rskip379(self) -> bool {
        RskHardforkConfig::mainnet().has_rskip379(self.block)
    }

    /// Whether the pegout index classifies the transaction.
    pub(super) fn indexed(self) -> bool {
        self.rskip379() && self.height >= INDEX_START
    }

    /// rskj `shouldMarkRejectedPeginAsProcessed`: RSKIP459 and not RSKIP551.
    pub(super) fn marks_refused(self) -> bool {
        let cfg = RskHardforkConfig::mainnet();
        cfg.has_rskip459(self.block) && !cfg.has_rskip551(self.block)
    }

    /// Mainnet's Bridge at the run's block, the active federation
    /// [`active`] in it, and with `with_retiring` the retiring one
    /// [`retiring`].
    pub(super) fn bridge(self, with_retiring: bool) -> Registration {
        let mut bridge = Registration::mainnet(self.block);
        bridge.federation(NEW_FEDERATION, &active());
        if with_retiring {
            bridge.federation(OLD_FEDERATION, &retiring());
        }
        bridge
    }
}

/// The active federation's members.
pub(super) fn active() -> Vec<[u8; 33]> {
    keys(7, 5)
}

/// The retiring federation's members.
pub(super) fn retiring() -> Vec<[u8; 33]> {
    keys(4, 3)
}

/// The last retired federation's members.
pub(super) fn retired() -> Vec<[u8; 33]> {
    keys(1, 3)
}

/// mainnet's `minimumPeginTxValue` from RSKIP219.
pub(super) fn minimum_pegin() -> u64 {
    BridgeConstants::mainnet().minimum_pegin_tx_value
}

/// mainnet's `minimumPegoutTxValue`.
pub(super) fn minimum_pegout() -> u64 {
    BridgeConstants::mainnet().minimum_pegout_tx_value
}

/// A transaction from a P2PKH sender paying `outputs`.
pub(super) fn from_a_user(outputs: &[(u64, Vec<u8>)]) -> bitcoin::Transaction {
    btc_transaction(vec![input(1, p2pkh_spend())], outputs)
}

/// A user's P2PKH address.
pub(super) fn user(tag: u8) -> Vec<u8> {
    p2pkh(&[tag; 20])
}

/// One bitcoin, in satoshis.
pub(super) const BTC: u64 = 100_000_000;
/// bitcoinj's `Coin.CENT`.
pub(super) const CENT: u64 = 1_000_000;

/// A peg-out or a migration: processed with no event, its outputs to the
/// active federation, `utxos` of them, registered.
pub(super) fn assert_moved(outcome: &Outcome, run: Run, utxos: usize) {
    assert!(outcome.returned, "{run:?}");
    assert!(outcome.events.is_empty(), "{run:?}");
    assert_eq!(outcome.processed_at, Some(run.block), "{run:?}");
    assert_eq!(outcome.active_utxos.len(), utxos, "{run:?}");
    assert!(outcome.retiring_utxos.is_empty(), "{run:?}");
}

/// A transaction spending `inputs` outputs of the federation of `keys`, the
/// first input's sighash in the pegout index from RSKIP379 as rskj's tests
/// mock it, registered in `run`.
pub(super) fn register_spend(
    run: Run,
    with_retiring: bool,
    keys: &[[u8; 33]],
    inputs: u8,
    outputs: &[(u64, Vec<u8>)],
) -> Outcome {
    let redeem = standard_redeem(keys);
    let tx = btc_transaction(
        (1..=inputs)
            .map(|tag| input(tag, multisig_spend(&redeem)))
            .collect(),
        outputs,
    );
    let mut bridge = run.bridge(with_retiring);
    if run.rskip379() {
        index_pegout(&mut bridge.ctx, &index_sighash(&tx, &redeem));
    }
    bridge.register(&tx, run.height)
}

/// rskj's forty outputs of its many-output peg-outs and migrations: a quarter
/// of the minimum peg-out, half of it, and the minimum, each and a cent, and
/// the minimum and a bitcoin, ten of each.
pub(super) fn forty_outputs(to: impl Fn(u8) -> Vec<u8>) -> Vec<(u64, Vec<u8>)> {
    let quarter = minimum_pegout() / 4;
    let values = [
        quarter + CENT,
        2 * quarter + CENT,
        minimum_pegout() + CENT,
        minimum_pegout() + BTC,
    ];
    (0..40u8)
        .map(|n| (values[usize::from(n / 10)], to(n)))
        .collect()
}

/// The logs the calls so far left, taken.
pub(super) fn logs(ctx: &mut Ctx) -> Vec<revm::primitives::Log> {
    ctx.journal_mut().take_logs()
}

/// A log's topics, as bytes.
pub(super) fn topics(log: &revm::primitives::Log) -> Vec<[u8; 32]> {
    log.data.topics().iter().map(|topic| topic.0).collect()
}

/// rskj's legacy event topic, `DataWord.fromString(name)`: the name's ASCII
/// right-aligned in a word.
pub(super) fn legacy_topic(name: &str) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[32 - name.len()..].copy_from_slice(name.as_bytes());
    word
}

/// A Solidity event's topic: the Keccak-256 of its signature.
pub(super) fn event_topic(signature: &str) -> [u8; 32] {
    use sha3::Digest;
    sha3::Keccak256::digest(signature.as_bytes()).into()
}

/// The `index`th word of an event's ABI data.
pub(super) fn word(data: &[u8], index: usize) -> U256 {
    U256::from_be_slice(&data[32 * index..32 * (index + 1)])
}

/// The hash of the RSK transaction that made the peg-out the federators
/// sign.
pub(super) const RELEASE_HASH: [u8; 32] = [0x2b; 32];

/// A peg-out waiting for its federation's signatures, as rskj's
/// `addSignature` tests keep one: the federation of the private keys 1, 2
/// and 3 active, and in `rskTxsWaitingFS`, under [`RELEASE_HASH`], a
/// transaction spending one of its outputs to a user, its input's scriptSig
/// the federation's with no signature yet
/// (`createBaseInputScriptThatSpendsFromTheFederation`). Members count in
/// the order of their keys in the redeem script.
pub(super) struct Signing {
    pub(super) bridge: Bridge,
    privates: Vec<[u8; 32]>,
    /// Where each member's transaction comes from: the address of its RSK
    /// key, which rskj's `addSignature` asks a sender to be a member's
    /// (`BridgeUtils.isFromFederateMember`).
    senders: Vec<Address>,
    pub(super) redeem: Vec<u8>,
    pub(super) unsigned: bitcoin::Transaction,
}

impl Signing {
    /// At block `number` of mainnet.
    pub(super) fn mainnet_at(number: u64) -> Self {
        Self::spending(number, 1)
    }

    /// At block `number` of mainnet, the peg-out spending `inputs` of the
    /// federation's outputs, each input's scriptSig the federation's with no
    /// signature yet.
    pub(super) fn spending(number: u64, inputs: u8) -> Self {
        use crate::bridge::federation::StoredMember;
        let members = (1..=3u8)
            .map(|i| {
                let mut private = [0u8; 32];
                private[31] = i;
                (StoredMember::from_btc(public_key(&[i])), private)
            })
            .collect();
        Self::of_members(number, members, inputs)
    }

    /// At block `number` of mainnet, the peg-out spending `inputs` of the
    /// outputs of a federation of `members`, each stored with its own RSK and
    /// MST keys and signing with the private key of its BTC key.
    pub(super) fn of_members(
        number: u64,
        mut members: Vec<(crate::bridge::federation::StoredMember, [u8; 32])>,
        inputs: u8,
    ) -> Self {
        members.sort();
        let publics: Vec<[u8; 33]> = members.iter().map(|(member, _)| member.btc).collect();
        let redeem = standard_redeem(&publics);
        let mut bridge = Bridge::mainnet_at(number);
        let stored: Vec<_> = members.iter().map(|(member, _)| *member).collect();
        store_members(&mut bridge.ctx, NEW_FEDERATION, &stored, 1, 1000);
        let threshold = usize::from(redeem[0] - 0x50);
        let script_sig = [vec![0x00; threshold + 1], push(&redeem)].concat();
        let spent = (1..=inputs)
            .map(|tag| input(tag, script_sig.clone()))
            .collect();
        let unsigned = btc_transaction(spent, &[(BTC, user(1))]);
        put_named(
            &mut bridge.ctx,
            "rskTxsWaitingFS",
            &rlp_encode_list(&[
                rlp_encode_element(&RELEASE_HASH),
                rlp_encode_element(&bitcoin::consensus::serialize(&unsigned)),
            ]),
        );
        let privates = members.iter().map(|(_, private)| *private).collect();
        let senders = members
            .iter()
            .map(|(member, _)| {
                crate::bridge::federation::rsk_address_from_public_key(&member.rsk)
                    .expect("an address")
            })
            .collect();
        Self {
            bridge,
            privates,
            senders,
            redeem,
            unsigned,
        }
    }

    /// A member's public key.
    pub(super) fn key(&self, member: usize) -> [u8; 33] {
        public_key(&self.privates[member])
    }

    /// What the peg-out's input `index` is signed over: its SIGHASH_ALL
    /// hash over the redeem script.
    pub(super) fn sighash(&self, index: usize) -> [u8; 32] {
        use bitcoin::hashes::Hash;
        bitcoin::sighash::SighashCache::new(&self.unsigned)
            .legacy_signature_hash(index, bitcoin::Script::from_bytes(&self.redeem), 1)
            .expect("a sighash")
            .to_byte_array()
    }

    /// The DER signature of the peg-out's input `index` by the private key
    /// `private`.
    pub(super) fn signature_by(&self, private: &[u8; 32], index: usize) -> Vec<u8> {
        use k256::ecdsa::signature::hazmat::PrehashSigner;
        let key = k256::ecdsa::SigningKey::from_slice(private).expect("a key");
        let signature: k256::ecdsa::Signature =
            key.sign_prehash(&self.sighash(index)).expect("a signature");
        signature.to_der().as_bytes().to_vec()
    }

    /// A member's DER signature of the peg-out's input `index`.
    pub(super) fn signature_of(&self, member: usize, index: usize) -> Vec<u8> {
        self.signature_by(&self.privates[member], index)
    }

    /// A member's DER signature of the peg-out's input, SIGHASH_ALL over
    /// the redeem script.
    pub(super) fn signature(&self, member: usize) -> Vec<u8> {
        self.signature_of(member, 0)
    }

    /// Another of a member's signatures of input `index`: one that verifies
    /// as [`Signing::signature_of`]'s does, made with another nonce.
    pub(super) fn another_signature_of(&self, member: usize, index: usize) -> Vec<u8> {
        use k256::ecdsa::signature::hazmat::RandomizedPrehashSigner;
        use rand::SeedableRng;
        let key = k256::ecdsa::SigningKey::from_slice(&self.privates[member]).expect("a key");
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let signature: k256::ecdsa::Signature = key
            .sign_prehash_with_rng(&mut rng, &self.sighash(index))
            .expect("a signature");
        signature.to_der().as_bytes().to_vec()
    }

    /// `addSignature` with `key` and `signatures`, called by the member's
    /// transaction.
    pub(super) fn add_signatures_as(
        &mut self,
        member: usize,
        key: &[u8],
        signatures: &[Vec<u8>],
    ) -> Result<PrecompileOutput, PrecompileError> {
        use crate::abi::encode::{args, Arg};
        self.bridge.caller = self.senders[member];
        let sent: Vec<&[u8]> = signatures.iter().map(Vec::as_slice).collect();
        let data = [
            crate::abi::selector("addSignature(bytes,bytes[],bytes)").to_vec(),
            args(&[
                Arg::Bytes(key),
                Arg::BytesArray(&sent),
                Arg::Bytes(&RELEASE_HASH),
            ]),
        ]
        .concat();
        self.bridge.execute(&data)
    }

    /// `addSignature` with the member's key and `signatures`, called by the
    /// member's transaction.
    pub(super) fn add_signatures(
        &mut self,
        member: usize,
        signatures: &[Vec<u8>],
    ) -> Result<PrecompileOutput, PrecompileError> {
        let key = self.key(member);
        self.add_signatures_as(member, &key, signatures)
    }

    /// `addSignature` with `signature`, called by the member's transaction.
    pub(super) fn add_signature(
        &mut self,
        member: usize,
        signature: &[u8],
    ) -> Result<PrecompileOutput, PrecompileError> {
        self.add_signatures(member, &[signature.to_vec()])
    }

    /// The member's own signature, as [`Signing::add_signature`] sends it.
    pub(super) fn sign(&mut self, member: usize) {
        let signature = self.signature(member);
        self.add_signature(member, &signature)
            .expect("addSignature runs");
    }

    /// The peg-out with the signatures of `members` in each input, as
    /// bitcoinj fills them in: in the redeem script's order, before the slots
    /// still empty.
    pub(super) fn signed_by(&self, members: &[usize]) -> bitcoin::Transaction {
        let threshold = usize::from(self.redeem[0] - 0x50);
        let mut sorted = members.to_vec();
        sorted.sort();
        let mut tx = self.unsigned.clone();
        for index in 0..tx.input.len() {
            let mut script_sig = vec![0x00];
            for &member in &sorted {
                script_sig.extend(push(
                    &[self.signature_of(member, index), vec![0x01]].concat(),
                ));
            }
            script_sig.extend(vec![0x00; threshold - sorted.len()]);
            script_sig.extend(push(&self.redeem));
            tx.input[index].script_sig = bitcoin::ScriptBuf::from_bytes(script_sig);
        }
        tx
    }
}

/// The votes of a majority of the network's federation-change authorizers,
/// at the Bridge's block, to create a pending federation, add `members` to
/// it (each key as its member's BTC, RSK and MST key), and commit it; what
/// the deciding commit answers.
pub(super) fn elect(bridge: &mut Bridge, members: &[[u8; 33]]) -> i128 {
    use crate::abi::encode::{args, Arg};
    use crate::bridge::governance;
    let all: Vec<Address> = bridge
        .constants
        .federation_change_authorizer_keys
        .iter()
        .map(|key| {
            let key = alloy_primitives::hex::decode(key).expect("a key in hex");
            crate::bridge::federation::rsk_address_from_public_key(&key).expect("a key")
        })
        .collect();
    let deciding = &all[..all.len() / 2 + 1];
    let sent_by = |sender: Address| BridgeTxContext {
        rsk_sender: sender,
        ..BridgeTxContext::default()
    };
    let (ctx, constants, cfg) = (&mut bridge.ctx, &bridge.constants, &bridge.cfg);
    for &sender in deciding {
        governance::create_federation(ctx, 0, constants, cfg, &sent_by(sender))
            .expect("createFederation answers");
    }
    for member in members {
        let member = args(&[Arg::Bytes(member), Arg::Bytes(member), Arg::Bytes(member)]);
        for &sender in deciding {
            governance::add_federator_public_key_multikey(
                ctx,
                &member,
                0,
                constants,
                cfg,
                &sent_by(sender),
            )
            .expect("addFederatorPublicKeyMultikey answers");
        }
    }
    let hash = crate::bridge::getters::get_pending_federation_hash(ctx, 0)
        .expect("getPendingFederationHash answers");
    let length = U256::from_be_slice(&hash.bytes[32..64]).to::<usize>();
    let hash = args(&[Arg::Bytes(&hash.bytes[64..64 + length])]);
    let mut answer = 0;
    for &sender in deciding {
        let out = governance::commit_federation(ctx, &hash, 0, constants, cfg, &sent_by(sender))
            .expect("commitFederation answers");
        answer = int256(&out.bytes);
    }
    answer
}

/// A UTXO of `satoshis` paid to `script`, its outpoint `[tag; 32]:0`.
pub(super) fn utxo(tag: u8, satoshis: u64, script: Vec<u8>) -> crate::bridge::storage::BridgeUtxo {
    crate::bridge::storage::BridgeUtxo {
        tx_hash: [tag; 32],
        vout: 0,
        value_satoshis: satoshis,
        height: 1,
        script,
        coinbase: false,
    }
}

/// Mainnet's federation activation age at block `number` (rskj
/// `FederationMainNetConstants`): 18,500 blocks before RSKIP383, 40,320 from
/// it.
pub(super) fn activation_age(number: u64) -> u64 {
    if RskHardforkConfig::mainnet().has_rskip383(number) {
        40_320
    } else {
        18_500
    }
}

/// Mainnet's `fundsMigrationAgeSinceActivationEnd`.
pub(super) const MIGRATION_AGE_END: u64 = 10_585;
/// rskj's `FederationFormatVersion.STANDARD_MULTISIG_FEDERATION`.
pub(super) const STANDARD_MULTISIG: u64 = 1000;
/// rskj's `FederationFormatVersion.P2SH_ERP_FEDERATION`.
pub(super) const P2SH_ERP: u64 = 3000;

/// rskj's `FederationFormatVersion.P2SH_P2WSH_ERP_FEDERATION`.
pub(super) const P2SH_P2WSH_ERP: u64 = 4000;

/// Mainnet's Bridge at a block, about to run `updateCollections` as rskj's
/// `updateCollections` tests set it up: a fee per kilobyte of a millicoin,
/// and the federations, UTXOs and requests a test gives it.
pub(super) struct Collections {
    pub(super) bridge: Bridge,
}

impl Collections {
    pub(super) fn mainnet_at(number: u64) -> Self {
        let mut bridge = Bridge::mainnet_at(number);
        put_named(
            &mut bridge.ctx,
            "feePerKb",
            &rlp_encode_element(&[0x01, 0x86, 0xa0]),
        );
        Self { bridge }
    }

    /// The Bridge at `number` with the retiring federation's funds to move:
    /// the active federation made, as rskj's tests make it, a block more
    /// than its activation age before, so that the migration has begun, or
    /// with `past` the migration's whole age before, so that it is over. The
    /// active federation holds ten UTXOs, the retiring one, stored in
    /// `format`, `utxos`.
    pub(super) fn migrating(number: u64, format: u64, utxos: u8, past: bool) -> Self {
        let age = activation_age(number) + if past { MIGRATION_AGE_END } else { 1 };
        let mut collections = Self::mainnet_at(number);
        collections.active(number - age, 10);
        collections.retiring(format, utxos);
        collections
    }

    /// The active federation, of [`active`]'s keys, made at block
    /// `creation`, with `utxos` UTXOs of a bitcoin.
    pub(super) fn active(&mut self, creation: u64, utxos: u8) {
        let keys = active();
        store_federation(
            &mut self.bridge.ctx,
            NEW_FEDERATION,
            &keys,
            creation,
            STANDARD_MULTISIG,
        );
        let script = address_of(&keys);
        let utxos: Vec<_> = (1..=utxos)
            .map(|tag| utxo(tag, BTC, script.clone()))
            .collect();
        let number = self.bridge.number();
        crate::bridge::storage::store_federation_utxos(
            &mut self.bridge.ctx,
            &self.bridge.cfg,
            number,
            &utxos,
        );
    }

    /// The retiring federation, of [`retiring`]'s keys stored in `format`,
    /// with `utxos` UTXOs of a bitcoin.
    pub(super) fn retiring(&mut self, format: u64, utxos: u8) {
        let keys = retiring();
        store_federation(&mut self.bridge.ctx, OLD_FEDERATION, &keys, 1, format);
        let script = address_of(&keys);
        let utxos: Vec<_> = (1..=utxos)
            .map(|tag| utxo(tag.wrapping_add(0x80), BTC, script.clone()))
            .collect();
        crate::bridge::storage::store_old_federation_utxos(&mut self.bridge.ctx, &utxos);
    }

    /// `count` peg-out requests of half a bitcoin, each with its
    /// transaction's hash.
    pub(super) fn requests(&mut self, count: u8) {
        let entries: Vec<([u8; 20], u64, [u8; 32])> =
            (1..=count).map(|i| ([i; 20], BTC / 2, [i; 32])).collect();
        put_named(
            &mut self.bridge.ctx,
            "releaseRequestQueueWithTxHash",
            &hashed_queue(&entries),
        );
    }

    /// `updateCollections`, and what it logged.
    pub(super) fn update(&mut self) -> Vec<revm::primitives::Log> {
        self.bridge.update_collections();
        logs(&mut self.bridge.ctx)
    }

    /// The peg-outs waiting for confirmations.
    pub(super) fn waiting(&mut self) -> Vec<bitcoin::Transaction> {
        crate::bridge::peg::load_pegout_confirmation_set(&mut self.bridge.ctx, true)
            .iter()
            .map(|entry| bitcoin::consensus::deserialize(&entry.btc_tx_raw).expect("a transaction"))
            .collect()
    }

    /// Whether the pegout index holds `tx`, its first input spending
    /// `redeem`.
    pub(super) fn indexed(&mut self, tx: &bitcoin::Transaction, redeem: &[u8]) -> bool {
        let sighash = alloy_primitives::hex::encode(index_sighash(tx, redeem));
        cell(&mut self.bridge.ctx, compound("pegoutTxSigHash", &sighash)) == Some(vec![1])
    }
}

/// A Bridge to call, as rskj's `BridgeSupportBuilder` builds one: a block of
/// a network, the transaction that reaches the Bridge, and what it carries.
pub(super) struct Bridge {
    pub(super) ctx: Ctx,
    pub(super) cfg: RskHardforkConfig,
    pub(super) constants: BridgeConstants,
    /// The transaction that reaches the Bridge.
    pub(super) tx: BridgeTxContext,
    /// Who calls: the transaction's sender.
    pub(super) caller: Address,
    /// What the call carries.
    pub(super) value: U256,
}

/// Nobody in particular: no federator, no authorizer.
pub(super) const STRANGER: Address = Address::repeat_byte(0x99);

impl Bridge {
    /// The Bridge at block `number` of `cfg`, under `constants`, called by a
    /// transaction of [`STRANGER`]'s.
    pub(super) fn new(cfg: RskHardforkConfig, constants: BridgeConstants, number: u64) -> Self {
        Self {
            ctx: ctx_at(number),
            cfg,
            constants,
            tx: BridgeTxContext {
                rsk_sender: STRANGER,
                rsk_tx_hash: [0x7e; 32],
                origin_tx_hash: [0x7e; 32],
                btc_sender_hash160: [0x5d; 20],
                ..Default::default()
            },
            caller: STRANGER,
            value: U256::ZERO,
        }
    }

    /// At block `number` of mainnet.
    pub(super) fn mainnet_at(number: u64) -> Self {
        Self::new(
            RskHardforkConfig::mainnet(),
            BridgeConstants::mainnet(),
            number,
        )
    }

    /// The number of the block the Bridge runs in.
    pub(super) fn number(&self) -> u64 {
        revm::context_interface::Block::number(self.ctx.block()).to::<u64>()
    }

    /// rskj's `bridge.execute(data)` for a transaction's own call, with all
    /// the gas it asks for. The call's writes are kept when it returns and
    /// dropped when it fails, as the executor keeps and drops them.
    pub(super) fn execute(&mut self, data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
        self.ctx.chain_mut().raw_storage.commit_call();
        let number = self.number();
        let checkpoint = self.ctx.journal_mut().checkpoint();
        let tx = BridgeTxContext {
            rsk_sender: self.caller,
            ..self.tx.clone()
        };
        let result = crate::bridge::execute_bridge(
            &mut self.ctx,
            data,
            u64::MAX,
            &self.constants,
            None,
            self.cfg.has_stored_block_v2(number),
            &self.cfg,
            &tx,
            self.caller,
            1,
            crate::bridge::BridgeCallKind::Call,
            self.value,
        );
        if result.is_ok() {
            self.ctx.chain_mut().raw_storage.commit_call();
            self.ctx.journal_mut().checkpoint_commit();
        } else {
            self.ctx.chain_mut().raw_storage.discard_call();
            self.ctx.journal_mut().checkpoint_revert(checkpoint);
        }
        result
    }

    /// rskj `BridgeSupport.releaseBtc` for `satoshis`, asked for by the
    /// Bridge's transaction, the value already at the Bridge as a call's value
    /// reaches its callee.
    pub(super) fn release(&mut self, satoshis: u64) {
        let wei = U256::from(satoshis) * U256::from(WEI_PER_SATOSHI);
        let held = self.balance(crate::precompiles::BRIDGE_ADDR);
        self.set_balance(crate::precompiles::BRIDGE_ADDR, held + wei);
        let tx = BridgeTxContext {
            rsk_sender: self.caller,
            ..self.tx.clone()
        };
        crate::bridge::peg::release_btc(
            &mut self.ctx,
            0,
            &self.constants,
            &self.cfg,
            &tx,
            self.caller,
            1,
            wei,
        )
        .expect("releaseBtc answers");
    }

    /// rskj `BridgeSupport.updateCollections`, called by the Bridge's
    /// transaction.
    pub(super) fn update_collections(&mut self) {
        let tx = BridgeTxContext {
            rsk_sender: self.caller,
            ..self.tx.clone()
        };
        crate::bridge::peg::update_collections(&mut self.ctx, 0, &self.constants, &self.cfg, &tx)
            .expect("updateCollections answers");
    }

    /// What `address` holds.
    pub(super) fn balance(&mut self, address: Address) -> U256 {
        use revm::context_interface::journaled_state::account::JournaledAccountTr;
        *self
            .ctx
            .journal_mut()
            .load_account_mut(address)
            .expect("an account")
            .balance()
    }

    /// Sets what `address` holds.
    pub(super) fn set_balance(&mut self, address: Address, amount: U256) {
        use revm::context_interface::journaled_state::account::JournaledAccountTr;
        self.ctx
            .journal_mut()
            .load_account_mut(address)
            .expect("an account")
            .set_balance(amount);
    }
}
