//! What this folder's ports share: a Bridge at an RSK block of an upgrade
//! ladder, a Bitcoin chain that holds a transaction, the transactions and
//! federations a peg-in is made of, and what a registration leaves behind.
//!
//! rskj's tests mock the Bitcoin block store
//! (`BridgeSupportTestUtil.mockChainOfStoredBlocks`). Here the blocks are
//! written into the Bridge's own store, as it keeps them ([`hold`]), so a
//! peg-in is registered at any Bitcoin height under any network's constants,
//! through `registerBtcTransaction` as a transaction calls it. What rskj
//! computes and rustock is held to (a script, an address, an event) is
//! computed here from rskj's definition, not by rustock's own code.

use alloy_primitives::{Address as RskAddress, B256, U256};
use bitcoin::hashes::Hash as _;
use revm::context_interface::journaled_state::account::JournaledAccountTr;
use revm::context_interface::{ContextTr, JournalTr};
use sha2::Digest as _;

use crate::abi::encode::{self, Arg};
use crate::bridge::btc_store::{self, StoredBlock};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::federation::{serialize_federation_multikey, StoredMember};
use crate::bridge::{btc_chain, peg, serialization, storage, tx};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::{BridgeTxContext, BRIDGE_ADDR};

/// One bitcoin, in satoshis (bitcoinj's `Coin.COIN`).
pub(super) const COIN: u64 = 100_000_000;

/// What one satoshi is worth in RBTC, in wei.
pub(super) const WEI_PER_SATOSHI: u64 = 10_000_000_000;

/// All the RBTC there is, in satoshis (rskj's `BridgeConstants.maxRbtc`).
pub(super) const MAX_RBTC: u64 = 21_000_000 * COIN;

/// The hash of the RSK transaction that calls the Bridge in these tests.
pub(super) const RSK_TX_HASH: [u8; 32] = [0x7e; 32];

/// The world a ported test calls the Bridge in: no trie and no database, only
/// what the test and the Bridge write.
pub(super) type Ctx = revm::Context<
    revm::context::BlockEnv,
    revm::context::TxEnv,
    revm::context::CfgEnv,
    revm::database::EmptyDB,
    revm::Journal<revm::database::EmptyDB>,
    crate::raw_storage::RskChainExt,
>;

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

/// rskj's `ActivationConfigsForTest.<last>()`: every upgrade up to `last`
/// active from block 0 and none after it, on the network `chain_id` names.
/// `Genesis` has nothing active, and `Vetiver900` is rskj's `all()`.
pub(super) fn up_to(last: RskNetworkUpgrade, chain_id: u64) -> RskHardforkConfig {
    LADDER
        .into_iter()
        .fold(RskHardforkConfig::all_active(chain_id), |cfg, upgrade| {
            cfg.with_hardfork(upgrade, (upgrade <= last).then_some(0))
        })
}

/// A Bridge with nothing stored, at RSK block `number`.
pub(super) fn ctx_at(number: u64) -> Ctx {
    use revm::MainContext;
    let mut ctx: Ctx =
        revm::Context::mainnet().with_chain(crate::raw_storage::RskChainExt::default());
    ctx.block.number = U256::from(number);
    ctx.block.timestamp = U256::from(1_700_000_000u64);
    ctx
}

/// The RSK block a context is at.
pub(super) fn block_of(ctx: &Ctx) -> u64 {
    ctx.block.number.to::<u64>()
}

/// rskj's `mockChainOfStoredBlocks`: the Bridge's Bitcoin store holds a block
/// at `height` whose merkle root is the hash of `raw`, so that a tree of `raw`
/// alone proves it there, and a block at each height above it up to `head`,
/// each on the one below, the last the store's head. Each is written as the
/// store keeps it at the context's block, and from RSKIP199 indexed by its
/// height.
pub(super) fn hold(ctx: &mut Ctx, raw: &[u8], height: u32, head: u32, cfg: &RskHardforkConfig) {
    hold_with_root(ctx, tx::calculate_btc_tx_hash(raw), height, head, cfg);
}

/// [`hold`], the block at `height` having `root` for its merkle root.
pub(super) fn hold_with_root(
    ctx: &mut Ctx,
    root: [u8; 32],
    height: u32,
    head: u32,
    cfg: &RskHardforkConfig,
) -> bitcoin::block::Header {
    hold_roots(ctx, &[root], height, head, cfg)[0]
}

/// [`hold`] of a block for each of `roots`, one on the other from `height`,
/// each having its root for its merkle root: their headers.
pub(super) fn hold_roots(
    ctx: &mut Ctx,
    roots: &[[u8; 32]],
    height: u32,
    head: u32,
    cfg: &RskHardforkConfig,
) -> Vec<bitcoin::block::Header> {
    let number = block_of(ctx);
    let (v2, rskip199) = (cfg.has_stored_block_v2(number), cfg.has_rskip199(number));
    let mut prev = bitcoin::BlockHash::from_byte_array([0x42; 32]);
    let mut held = Vec::new();
    for at in height..=head {
        let root = roots.get((at - height) as usize);
        let header = bitcoin::block::Header {
            version: bitcoin::block::Version::from_consensus(1),
            prev_blockhash: prev,
            merkle_root: bitcoin::TxMerkleNode::from_byte_array(
                root.copied().unwrap_or([0x11; 32]),
            ),
            time: 1_700_000_000,
            bits: bitcoin::CompactTarget::from_consensus(0x1d00_ffff),
            nonce: at,
        };
        let block = StoredBlock::new(header, at, U256::from(at));
        btc_store::put_stored_block(ctx, &block, v2);
        if rskip199 {
            storage::bridge_store_btc_block_hash_by_height(
                ctx,
                at,
                btc_chain::bitcoin_hash_to_b256(&header.block_hash()),
            );
        }
        if at == head {
            btc_store::store_chain_head(ctx, &block, v2);
        }
        if root.is_some() {
            held.push(header);
        }
        prev = header.block_hash();
    }
    assert_eq!(
        held.len(),
        roots.len(),
        "a block for each root, at or below the head"
    );
    held
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

/// An event as the Bridge logs it: its topics, then its data.
pub(super) type Event = (Vec<B256>, Vec<u8>);

/// What `registerBtcTransaction` answered and left behind.
#[derive(Debug, PartialEq)]
pub(super) struct Registered {
    /// The call returned, rather than threw.
    pub(super) returned: bool,
    /// The transaction is marked processed.
    pub(super) processed: bool,
    /// The events it logged.
    pub(super) logs: Vec<Event>,
}

/// `registerBtcTransaction` of `tx` at Bitcoin height `height`, in the
/// transaction [`RSK_TX_HASH`]. Its writes are kept when it returns and
/// dropped when it throws, as the executor keeps and drops them; what was
/// written before it is the state it starts from, and stays.
pub(super) fn register(
    ctx: &mut Ctx,
    tx: &bitcoin::Transaction,
    height: i64,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) -> Registered {
    register_bytes(
        ctx,
        &bitcoin::consensus::serialize(tx),
        tx,
        height,
        config,
        cfg,
    )
}

/// [`register`], with `tx` given to the Bridge as the bytes `raw`.
pub(super) fn register_bytes(
    ctx: &mut Ctx,
    raw: &[u8],
    tx: &bitcoin::Transaction,
    height: i64,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) -> Registered {
    let tree = one_leaf_tree(raw);
    let returned = register_with_tree(ctx, raw, height, &tree, config, cfg).is_ok();
    let logs = take_logs(ctx);
    let processed = tx::is_btc_tx_processed(ctx, &tx::legacy_btc_txid(tx));
    Registered {
        returned,
        processed,
        logs,
    }
}

/// `registerBtcTransaction` of the bytes `raw` at `height` with the tree
/// `tree`, as [`register`] calls it: what it answered.
pub(super) fn register_with_tree(
    ctx: &mut Ctx,
    raw: &[u8],
    height: i64,
    tree: &[u8],
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) -> Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError> {
    let args = encode::args(&[
        Arg::Bytes(raw),
        Arg::Word(encode::int(height)),
        Arg::Bytes(tree),
    ]);
    in_a_call(ctx, |ctx| {
        peg::register_btc_transaction(ctx, &args, 0, config, cfg, &tx_context())
    })
}

/// The transaction [`RSK_TX_HASH`], as the Bridge sees the call it makes.
pub(super) fn tx_context() -> BridgeTxContext {
    BridgeTxContext {
        rsk_tx_hash: RSK_TX_HASH,
        origin_tx_hash: RSK_TX_HASH,
        ..BridgeTxContext::default()
    }
}

/// `call`, run as a transaction runs a Bridge method: what it writes is kept
/// when it returns and dropped when it throws, as the executor keeps and
/// drops them.
pub(super) fn in_a_call<T, E>(
    ctx: &mut Ctx,
    call: impl FnOnce(&mut Ctx) -> Result<T, E>,
) -> Result<T, E> {
    ctx.chain_mut().raw_storage.commit_call();
    let checkpoint = ctx.journal_mut().checkpoint();
    let result = call(ctx);
    if result.is_ok() {
        ctx.chain_mut().raw_storage.commit_call();
        ctx.journal_mut().checkpoint_commit();
    } else {
        ctx.chain_mut().raw_storage.discard_call();
        ctx.journal_mut().checkpoint_revert(checkpoint);
    }
    result
}

/// The events logged since the last time they were taken.
pub(super) fn take_logs(ctx: &mut Ctx) -> Vec<Event> {
    ctx.journal_mut()
        .take_logs()
        .into_iter()
        .map(|log| (log.data.topics().to_vec(), log.data.data.to_vec()))
        .collect()
}

/// A Bridge at RSK block `number` whose Bitcoin store holds `tx` at `height`
/// with as many confirmations as `config` asks of a peg-in, and which holds
/// all the RBTC there is: the federations hold nothing yet, so no peg-in here
/// meets the locking cap unless a test sets it to.
pub(super) fn bridge_holding(
    tx: &bitcoin::Transaction,
    height: u32,
    number: u64,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) -> Ctx {
    let mut ctx = ctx_at(number);
    let raw = bitcoin::consensus::serialize(tx);
    let head = height + config.btc2rsk_minimum_acceptable_confirmations - 1;
    hold(&mut ctx, &raw, height, head, cfg);
    fund_bridge_but(&mut ctx, 0);
    ctx
}

/// The peg-outs waiting for confirmations: a refund a registration made is
/// one of them.
pub(super) fn waiting_pegouts(ctx: &mut Ctx) -> Vec<bitcoin::Transaction> {
    peg::load_pegout_confirmation_set(ctx, true)
        .into_iter()
        .map(|entry| bitcoin::consensus::deserialize(&entry.btc_tx_raw).expect("a transaction"))
        .collect()
}

/// What `address` holds, in wei.
pub(super) fn balance(ctx: &mut Ctx, address: RskAddress) -> U256 {
    *ctx.journal_mut()
        .load_account_mut(address)
        .expect("the account")
        .balance()
}

/// Gives the Bridge all the RBTC there is but `locked` satoshis, which the
/// federations hold for it: rskj's locking cap counts what the Bridge has
/// paid out, all the RBTC there is less what it holds.
pub(super) fn fund_bridge_but(ctx: &mut Ctx, locked: u64) {
    let wei = U256::from(MAX_RBTC - locked) * U256::from(WEI_PER_SATOSHI);
    ctx.journal_mut()
        .load_account_mut(BRIDGE_ADDR)
        .expect("the Bridge's account")
        .set_balance(wei);
}

/// `satoshis` in wei.
pub(super) fn wei(satoshis: u64) -> U256 {
    U256::from(satoshis) * U256::from(WEI_PER_SATOSHI)
}

/// A thousandth of a bitcoin (bitcoinj's `Coin.MILLICOIN`): the fee per
/// kilobyte rskj's tests have the Bridge pay.
pub(super) const MILLICOIN: u64 = 100_000;

/// Sets the fee per kilobyte the Bridge pays its Bitcoin transactions with.
pub(super) fn set_fee_per_kb(ctx: &mut Ctx, satoshis: u64) {
    storage::bridge_store_u256(ctx, storage::FEE_PER_KB_KEY, U256::from(satoshis));
}

// ---------------------------------------------------------------------------
// Keys, hashes and scripts
// ---------------------------------------------------------------------------

/// The compressed public key of the private key `private`, read as rskj's
/// `BtcECKey.fromPrivate` reads it: a big-endian number.
pub(super) fn public_key(private: &[u8]) -> [u8; 33] {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    let mut scalar = [0u8; 32];
    scalar[32 - private.len()..].copy_from_slice(private);
    let key = k256::SecretKey::from_slice(&scalar).expect("a private key in range");
    key.public_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// rskj's `BitcoinTestUtils.getBtcEcKeyFromSeed`: the key whose private key
/// is the seed's Keccak-256 hash.
pub(super) fn key_from_seed(seed: &str) -> [u8; 33] {
    use sha3::Digest as _;
    public_key(&sha3::Keccak256::digest(seed.as_bytes()))
}

/// rskj's `ECKey.getAddress`: the last 20 bytes of the Keccak-256 hash of a
/// key's uncompressed point.
pub(super) fn rsk_address_of(key: &[u8; 33]) -> RskAddress {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    use sha3::Digest as _;
    let point = k256::PublicKey::from_sec1_bytes(key).expect("a public key");
    let uncompressed = point.to_encoded_point(false);
    RskAddress::from_slice(&sha3::Keccak256::digest(&uncompressed.as_bytes()[1..])[12..])
}

/// RIPEMD-160 of SHA-256, Bitcoin's hash of a key or a script.
pub(super) fn hash160(data: &[u8]) -> [u8; 20] {
    use ripemd::Digest as _;
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}

/// `OP_DUP OP_HASH160 <hash> OP_EQUALVERIFY OP_CHECKSIG`.
pub(super) fn p2pkh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0x76, 0xa9, 20];
    script.extend_from_slice(hash);
    script.extend_from_slice(&[0x88, 0xac]);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// `OP_HASH160 <hash> OP_EQUAL`.
pub(super) fn p2sh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0xa9, 20];
    script.extend_from_slice(hash);
    script.push(0x87);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// `OP_0 <hash>`: a native segwit (bech32) output, P2WPKH.
pub(super) fn p2wpkh(hash: &[u8; 20]) -> bitcoin::ScriptBuf {
    let mut script = vec![0x00, 20];
    script.extend_from_slice(hash);
    bitcoin::ScriptBuf::from_bytes(script)
}

/// The P2SH output of a redeem script.
pub(super) fn p2sh_of(redeem: &[u8]) -> bitcoin::ScriptBuf {
    p2sh(&hash160(redeem))
}

/// The witness program of a redeem script, `OP_0 <sha256(redeem)>`.
pub(super) fn witness_program(redeem: &[u8]) -> Vec<u8> {
    let mut program = vec![0x00, 32];
    program.extend_from_slice(&sha2::Sha256::digest(redeem));
    program
}

/// The P2SH-P2WSH output of a redeem script: the P2SH of its witness
/// program.
pub(super) fn p2sh_p2wsh_of(redeem: &[u8]) -> bitcoin::ScriptBuf {
    p2sh(&hash160(&witness_program(redeem)))
}

/// bitcoinj-thin's `ScriptBuilder.createRedeemScript(threshold, keys)`: the
/// keys sorted, and the counts as `ScriptBuilder.number` writes them, OP_1 to
/// OP_15 below 16 and a one-byte push from 16 on.
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

/// A standard multisig federation's redeem script: `size / 2 + 1` of its
/// members.
pub(super) fn standard_redeem(keys: &[[u8; 33]]) -> Vec<u8> {
    multisig(keys, keys.len() / 2 + 1)
}

/// rskj's `P2shErpRedeemScriptBuilder`, which a P2SH-P2WSH ERP federation's
/// script is built by too: `OP_NOTIF <members' multisig> OP_ELSE <delay>
/// OP_CHECKSEQUENCEVERIFY OP_DROP <emergency multisig> OP_ENDIF`, the delay as
/// bitcoinj-thin's `Utils.signedLongToByteArrayLE` writes it.
pub(super) fn erp_redeem(members: &[[u8; 33]], emergency: &[[u8; 33]], delay: i64) -> Vec<u8> {
    // `BigInteger.valueOf(delay).toByteArray()`: big-endian, as few bytes as
    // hold it with a sign bit, then reversed.
    let mut csv: Vec<u8> = delay
        .to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect();
    if csv.first().is_none_or(|byte| byte & 0x80 != 0) {
        csv.insert(0, 0);
    }
    csv.reverse();
    let mut script = vec![0x64];
    script.extend(standard_redeem(members));
    script.push(0x67);
    script.push(csv.len() as u8);
    script.extend(csv);
    script.extend_from_slice(&[0xb2, 0x75]);
    script.extend(standard_redeem(emergency));
    script.push(0x68);
    script
}

/// The keys of a network's Bridge constants, written in hex.
pub(super) fn keys_of(hexes: &[&str]) -> Vec<[u8; 33]> {
    hexes
        .iter()
        .map(|key| {
            hex::decode(key)
                .expect("a key in hex")
                .try_into()
                .expect("33 bytes")
        })
        .collect()
}

/// The output a network's genesis federation is paid at, built as rskj's
/// `FederationSupportImpl.getGenesisFederation` builds it: a standard multisig
/// on mainnet and testnet, a P2SH-P2WSH ERP federation on rskj's regtest.
pub(super) fn genesis_federation_output(config: &BridgeConstants) -> bitcoin::ScriptBuf {
    let members = keys_of(config.genesis_federation_public_keys);
    match config.genesis_federation_format {
        4000 => p2sh_p2wsh_of(&erp_redeem(
            &members,
            &keys_of(config.erp_fed_pubkeys),
            config.erp_fed_activation_delay,
        )),
        _ => p2sh_of(&standard_redeem(&members)),
    }
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

/// Stores a federation of `keys`, one key each, in rskj's multikey format,
/// created at RSK block `creation_block`, with its format version.
pub(super) fn store_federation(
    ctx: &mut Ctx,
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

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

/// A signature's worth of bytes: no node checks a peg-in's signatures.
const SIGNATURE: [u8; 71] = [0x30; 71];

/// The scriptSig of a P2PKH spend of `key`'s output: a signature, then the
/// key.
pub(super) fn p2pkh_spend(key: &[u8; 33]) -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    let mut script_sig = vec![SIGNATURE.len() as u8];
    script_sig.extend_from_slice(&SIGNATURE);
    script_sig.push(33);
    script_sig.extend_from_slice(key);
    (
        bitcoin::ScriptBuf::from_bytes(script_sig),
        bitcoin::Witness::new(),
    )
}

/// A P2SH-P2WPKH spend of `key`'s output: the scriptSig pushes the witness
/// program `OP_0 <hash160(key)>`, and the witness holds a signature and the
/// key.
pub(super) fn p2sh_p2wpkh_spend(key: &[u8; 33]) -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    let mut program = vec![0x00, 20];
    program.extend_from_slice(&hash160(key));
    let mut script_sig = vec![program.len() as u8];
    script_sig.extend(program);
    let mut witness = bitcoin::Witness::new();
    witness.push(SIGNATURE);
    witness.push(key);
    (bitcoin::ScriptBuf::from_bytes(script_sig), witness)
}

/// A P2SH spend of a multisig redeem script: `OP_0`, `signatures`
/// signatures, then the redeem script.
pub(super) fn p2sh_multisig_spend(
    redeem: &[u8],
    signatures: usize,
) -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    let mut script_sig = vec![0x00];
    for _ in 0..signatures {
        script_sig.push(SIGNATURE.len() as u8);
        script_sig.extend_from_slice(&SIGNATURE);
    }
    script_sig.extend(push(redeem));
    (
        bitcoin::ScriptBuf::from_bytes(script_sig),
        bitcoin::Witness::new(),
    )
}

/// A P2SH-P2WSH spend of a multisig redeem script: the scriptSig pushes the
/// witness program, and the witness holds an empty item, `signatures`
/// signatures and the redeem script.
pub(super) fn p2sh_p2wsh_spend(
    redeem: &[u8],
    signatures: usize,
) -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    let program = witness_program(redeem);
    let mut script_sig = vec![program.len() as u8];
    script_sig.extend(program);
    let mut witness = bitcoin::Witness::new();
    witness.push([0u8; 0]);
    for _ in 0..signatures {
        witness.push(SIGNATURE);
    }
    witness.push(redeem);
    (bitcoin::ScriptBuf::from_bytes(script_sig), witness)
}

/// An input with nothing in its scriptSig and no witness, from which rskj
/// names no sender (rskj's tests' `new Script(new byte[]{})`).
pub(super) fn no_sender() -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    (bitcoin::ScriptBuf::new(), bitcoin::Witness::new())
}

/// A native P2WPKH spend of `key`'s output: nothing in the scriptSig, a
/// signature and the key in the witness. rskj names no sender for it.
pub(super) fn p2wpkh_spend(key: &[u8; 33]) -> (bitcoin::ScriptBuf, bitcoin::Witness) {
    let mut witness = bitcoin::Witness::new();
    witness.push(SIGNATURE);
    witness.push(key);
    (bitcoin::ScriptBuf::new(), witness)
}

/// A push of `data`, as bitcoinj-thin's `ScriptBuilder.data` writes one.
pub(super) fn push(data: &[u8]) -> Vec<u8> {
    let mut out = match data.len() {
        len @ 0..=75 => vec![len as u8],
        len @ 76..=255 => vec![0x4c, len as u8],
        len => {
            let mut prefix = vec![0x4d];
            prefix.extend_from_slice(&(len as u16).to_le_bytes());
            prefix
        }
    };
    out.extend_from_slice(data);
    out
}

/// A version 1 transaction (bitcoinj's default) with `inputs`, each spending
/// an output of its own, paying `outputs`.
pub(super) fn tx(
    inputs: Vec<(bitcoin::ScriptBuf, bitcoin::Witness)>,
    outputs: &[(u64, bitcoin::ScriptBuf)],
) -> bitcoin::Transaction {
    bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: inputs
            .into_iter()
            .enumerate()
            .map(|(index, (script_sig, witness))| bitcoin::TxIn {
                previous_output: bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_byte_array([0x5a; 32]),
                    vout: index as u32,
                },
                script_sig,
                sequence: bitcoin::Sequence::MAX,
                witness,
            })
            .collect(),
        output: outputs
            .iter()
            .map(|(value, script_pubkey)| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(*value),
                script_pubkey: script_pubkey.clone(),
            })
            .collect(),
    }
}

/// An `OP_RETURN` output's script, pushing `data`.
pub(super) fn op_return(data: &[u8]) -> bitcoin::ScriptBuf {
    let mut script = vec![0x6a];
    script.extend(push(data));
    bitcoin::ScriptBuf::from_bytes(script)
}

/// Where a peg-in's instructions ask a refund to go.
pub(super) enum Refund {
    P2pkh([u8; 20]),
    P2sh([u8; 20]),
}

/// rskj's `PegTestUtils.createOpReturnScriptForRsk`: `RSKT`, the version,
/// the RSK destination, and where a refund goes, its type (1 for P2PKH, 2 for
/// P2SH) then its hash.
pub(super) fn rskt(
    version: u8,
    destination: RskAddress,
    refund: Option<Refund>,
) -> bitcoin::ScriptBuf {
    let mut payload = b"RSKT".to_vec();
    payload.push(version);
    payload.extend_from_slice(destination.as_slice());
    match refund {
        Some(Refund::P2pkh(hash)) => {
            payload.push(1);
            payload.extend_from_slice(&hash);
        }
        Some(Refund::P2sh(hash)) => {
            payload.push(2);
            payload.extend_from_slice(&hash);
        }
        None => {}
    }
    op_return(&payload)
}

/// Whitelists `hash` once for up to `max` satoshis, as rskj's
/// `OneOffWhiteListEntry`, with the whitelist on for good.
pub(super) fn whitelist_once(ctx: &mut Ctx, hash: [u8; 20], max: u64) {
    let (mut entries, disable_height) = storage::load_one_off_whitelist(ctx);
    entries.push((hash, max));
    storage::store_one_off_whitelist(ctx, &entries, disable_height);
}

/// Whether `hash` is on the one-off whitelist.
pub(super) fn whitelisted_once(ctx: &mut Ctx, hash: &[u8; 20]) -> bool {
    storage::load_one_off_whitelist(ctx)
        .0
        .iter()
        .any(|(entry, _)| entry == hash)
}

// ---------------------------------------------------------------------------
// Events, as rskj's `BridgeEvents` defines them
// ---------------------------------------------------------------------------

/// The first topic of the event `signature`.
pub(super) fn topic(signature: &str) -> B256 {
    alloy_primitives::keccak256(signature.as_bytes())
}

/// How many of `logs` are the event `signature`.
pub(super) fn count_of(logs: &[Event], signature: &str) -> usize {
    logs.iter()
        .filter(|(topics, _)| topics.first() == Some(&topic(signature)))
        .count()
}

fn word(n: u64) -> [u8; 32] {
    U256::from(n).to_be_bytes()
}

fn address_word(address: RskAddress) -> B256 {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(address.as_slice());
    B256::from(word)
}

/// `btcTx.getHash().getBytes()`: the transaction's id, without its witness,
/// in the order it is displayed.
pub(super) fn txid_bytes(tx: &bitcoin::Transaction) -> [u8; 32] {
    let mut bytes = tx.compute_txid().to_byte_array();
    bytes.reverse();
    bytes
}

/// `rejected_pegin(bytes32 indexed btcTxHash, int256 reason)`. rskj's
/// `RejectedPeginReason`: PEGIN_CAP_SURPASSED 1, LEGACY_PEGIN_MULTISIG_SENDER
/// 2, LEGACY_PEGIN_UNDETERMINED_SENDER 3, PEGIN_V1_INVALID_PAYLOAD 4,
/// INVALID_AMOUNT 5.
pub(super) fn rejected_pegin(tx: &bitcoin::Transaction, reason: u64) -> Event {
    (
        vec![
            topic("rejected_pegin(bytes32,int256)"),
            B256::from(txid_bytes(tx)),
        ],
        word(reason).to_vec(),
    )
}

/// `unrefundable_pegin(bytes32 indexed btcTxHash, int256 reason)`. rskj's
/// `NonRefundablePeginReason`: LEGACY_PEGIN_UNDETERMINED_SENDER 1,
/// PEGIN_V1_REFUND_ADDRESS_NOT_SET 2, INVALID_AMOUNT 3,
/// OUTPUTS_SENT_TO_DIFFERENT_TYPES_OF_FEDS 4.
pub(super) fn unrefundable_pegin(tx: &bitcoin::Transaction, reason: u64) -> Event {
    (
        vec![
            topic("unrefundable_pegin(bytes32,int256)"),
            B256::from(txid_bytes(tx)),
        ],
        word(reason).to_vec(),
    )
}

/// `pegin_btc(address indexed receiver, bytes32 indexed btcTxHash, int256
/// amount, int256 protocolVersion)`, the amount in satoshis.
pub(super) fn pegin_btc(
    receiver: RskAddress,
    tx: &bitcoin::Transaction,
    amount: u64,
    version: u64,
) -> Event {
    let mut data = word(amount).to_vec();
    data.extend_from_slice(&word(version));
    (
        vec![
            topic("pegin_btc(address,bytes32,int256,int256)"),
            address_word(receiver),
            B256::from(txid_bytes(tx)),
        ],
        data,
    )
}

/// `lock_btc(address indexed receiver, bytes32 btcTxHash, string
/// senderBtcAddress, int256 amount)`, the amount in satoshis.
pub(super) fn lock_btc(
    receiver: RskAddress,
    tx: &bitcoin::Transaction,
    sender: &str,
    amount: u64,
) -> Event {
    let mut data = txid_bytes(tx).to_vec();
    data.extend_from_slice(&word(96));
    data.extend_from_slice(&word(amount));
    data.extend_from_slice(&word(sender.len() as u64));
    data.extend_from_slice(sender.as_bytes());
    data.resize(data.len().div_ceil(32) * 32, 0);
    (
        vec![
            topic("lock_btc(address,bytes32,string,int256)"),
            address_word(receiver),
        ],
        data,
    )
}

/// `release_requested(bytes32 indexed rskTxHash, bytes32 indexed btcTxHash,
/// uint256 amount)`, for the refund `refund` of [`RSK_TX_HASH`]'s call.
pub(super) fn release_requested(refund: &bitcoin::Transaction, amount: u64) -> Event {
    (
        vec![
            topic("release_requested(bytes32,bytes32,uint256)"),
            B256::from(RSK_TX_HASH),
            B256::from(txid_bytes(refund)),
        ],
        word(amount).to_vec(),
    )
}

/// `pegout_confirmed(bytes32 indexed btcTxHash, uint256
/// pegoutCreationRskBlockNumber)`.
pub(super) fn pegout_confirmed(pegout: &bitcoin::Transaction, creation_block: u64) -> Event {
    (
        vec![
            topic("pegout_confirmed(bytes32,uint256)"),
            B256::from(txid_bytes(pegout)),
        ],
        word(creation_block).to_vec(),
    )
}

/// The base58 address of a hash: rskj's `Address.toString()`.
pub(super) fn base58(version: u8, hash: &[u8; 20]) -> String {
    let mut payload = vec![version];
    payload.extend_from_slice(hash);
    bitcoin::base58::encode_check(&payload)
}
