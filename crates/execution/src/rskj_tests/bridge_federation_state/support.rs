//! What this folder's ports share: a Bridge at an RSK block, the votes that
//! change its federation, the cells it keeps them in, and what rskj writes in
//! them, written here from rskj's definitions (`BridgeSerializationUtils`,
//! `FederationMember.serialize`, the redeem script builders) rather than by
//! rustock's own code.

use alloy_primitives::{Address as RskAddress, B256, U256};
use revm::context_interface::{ContextTr, JournalTr};
use sha2::Digest as _;

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{getters, governance, storage};
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::BridgeTxContext;

/// The world a ported test calls the Bridge in: no trie and no database.
pub(super) type Ctx = revm::Context<
    revm::context::BlockEnv,
    revm::context::TxEnv,
    revm::context::CfgEnv,
    revm::database::EmptyDB,
    revm::Journal<revm::database::EmptyDB>,
    crate::raw_storage::RskChainExt,
>;

/// The timestamp of the RSK blocks here, rskj's `RSK_EXECUTION_BLOCK_TIMESTAMP`
/// in `VoteFederationChangeTest`.
pub(super) const TIMESTAMP: u64 = 10;

/// rskj's mainnet genesis federation's creation time, 1970-01-18T12:49:08.400Z
/// (`FederationMainNetConstants`), in milliseconds.
pub(super) const MAINNET_GENESIS_CREATION_MILLIS: u64 = 1_514_948_400;

/// A Bridge with nothing stored, at RSK block `number`.
pub(super) fn ctx_at(number: u64) -> Ctx {
    use revm::MainContext;
    let mut ctx: Ctx =
        revm::Context::mainnet().with_chain(crate::raw_storage::RskChainExt::default());
    ctx.block.number = U256::from(number);
    ctx.block.timestamp = U256::from(TIMESTAMP);
    ctx
}

/// The bytes the Bridge keeps in its cell `name`.
pub(super) fn cell(ctx: &mut Ctx, name: &str) -> Vec<u8> {
    storage::bridge_load_bytes_named(ctx, name)
}

/// Writes `value` in the Bridge's cell `name`.
pub(super) fn put(ctx: &mut Ctx, name: &str, value: &[u8]) {
    storage::bridge_store_bytes_named(ctx, name, value);
}

// ---------------------------------------------------------------------------
// Keys and scripts
// ---------------------------------------------------------------------------

/// The compressed public key of the private key `private`, a big-endian
/// number.
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

/// `count` members' keys, of the private keys 0x21, 0x22, and so on,
/// sorted as rskj sorts a federation's members.
pub(super) fn members(count: u8) -> Vec<[u8; 33]> {
    let mut keys: Vec<[u8; 33]> = (0..count).map(|i| public_key(&[0x21 + i])).collect();
    keys.sort();
    keys
}

/// The keys of a network's constants, written in hex, sorted.
pub(super) fn keys_of(hexes: &[&str]) -> Vec<[u8; 33]> {
    let mut keys: Vec<[u8; 33]> = hexes
        .iter()
        .map(|key| hex::decode(key).expect("hex").try_into().expect("33 bytes"))
        .collect();
    keys.sort();
    keys
}

/// The RSK address of a public key, compressed or not: the last 20 bytes of
/// the Keccak-256 hash of its uncompressed point.
pub(super) fn rsk_address_of(key: &[u8]) -> RskAddress {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    use sha3::Digest as _;
    let point = k256::PublicKey::from_sec1_bytes(key).expect("a public key");
    let uncompressed = point.to_encoded_point(false);
    RskAddress::from_slice(&sha3::Keccak256::digest(&uncompressed.as_bytes()[1..])[12..])
}

/// RIPEMD-160 of SHA-256.
pub(super) fn hash160(data: &[u8]) -> [u8; 20] {
    use ripemd::Digest as _;
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}

/// bitcoinj-thin's `ScriptBuilder.createRedeemScript`: the keys sorted, the
/// counts as `ScriptBuilder.number` writes them.
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

/// A standard multisig federation's redeem script, `size / 2 + 1` of its
/// members.
pub(super) fn standard_redeem(keys: &[[u8; 33]]) -> Vec<u8> {
    multisig(keys, keys.len() / 2 + 1)
}

/// bitcoinj-thin's `Utils.signedLongToByteArrayLE`: Java's
/// `BigInteger.toByteArray`, reversed.
pub(super) fn signed_le(n: i64) -> Vec<u8> {
    let mut bytes: Vec<u8> = n
        .to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect();
    if bytes.first().is_none_or(|byte| byte & 0x80 != 0) {
        bytes.insert(0, 0);
    }
    bytes.reverse();
    bytes
}

/// rskj's `P2shErpRedeemScriptBuilder`, which the P2SH-P2WSH ERP federations'
/// scripts are built by too: `OP_NOTIF <members> OP_ELSE <delay>
/// OP_CHECKSEQUENCEVERIFY OP_DROP <emergency> OP_ENDIF`.
pub(super) fn p2sh_erp_redeem(members: &[[u8; 33]], emergency: &[[u8; 33]], delay: i64) -> Vec<u8> {
    let csv = signed_le(delay);
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

/// rskj's non-standard ERP redeem script (`NonStandardErpRedeemScriptBuilder`):
/// the two multisigs without their `OP_CHECKMULTISIG`, and one after
/// `OP_ENDIF`. `delay` is the bytes the delay is pushed as.
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

/// The hash a P2SH output of `redeem` pays, as a P2SH address holds it.
pub(super) fn p2sh_hash(redeem: &[u8]) -> [u8; 20] {
    hash160(redeem)
}

/// The hash a P2SH-P2WSH output of `redeem` pays: its witness program's.
pub(super) fn p2sh_p2wsh_hash(redeem: &[u8]) -> [u8; 20] {
    let mut program = vec![0x00, 32];
    program.extend_from_slice(&sha2::Sha256::digest(redeem));
    hash160(&program)
}

/// `OP_HASH160 <hash> OP_EQUAL`.
pub(super) fn p2sh_script(hash: &[u8; 20]) -> Vec<u8> {
    let mut script = vec![0xa9, 20];
    script.extend_from_slice(hash);
    script.push(0x87);
    script
}

/// A mainnet P2SH address, base58 with version 5.
pub(super) fn mainnet_p2sh_address(hash: &[u8; 20]) -> String {
    let mut payload = vec![5];
    payload.extend_from_slice(hash);
    bitcoin::base58::encode_check(&payload)
}

// ---------------------------------------------------------------------------
// What rskj writes (`BridgeSerializationUtils`, RLP as ethereumj encodes it)
// ---------------------------------------------------------------------------

/// `RLP.encodeElement`.
pub(super) fn rlp_element(data: &[u8]) -> Vec<u8> {
    let mut out = match data.len() {
        0 => return vec![0x80],
        1 if data[0] < 0x80 => return data.to_vec(),
        len if len < 56 => vec![0x80 + len as u8],
        len => {
            let size: Vec<u8> = (len as u64)
                .to_be_bytes()
                .into_iter()
                .skip_while(|byte| *byte == 0)
                .collect();
            let mut prefix = vec![0xb7 + size.len() as u8];
            prefix.extend(size);
            prefix
        }
    };
    out.extend_from_slice(data);
    out
}

/// `RLP.encodeList`.
pub(super) fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = items.concat();
    let mut out = if payload.len() < 56 {
        vec![0xc0 + payload.len() as u8]
    } else {
        let size: Vec<u8> = (payload.len() as u64)
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        let mut prefix = vec![0xf7 + size.len() as u8];
        prefix.extend(size);
        prefix
    };
    out.extend(payload);
    out
}

/// `RLP.encodeBigInteger(BigInteger.valueOf(n))`, which
/// `serializeInteger` and `serializeLong` write.
pub(super) fn rlp_number(n: u64) -> Vec<u8> {
    if n == 0 {
        return vec![0x80];
    }
    let bytes: Vec<u8> = n
        .to_be_bytes()
        .into_iter()
        .skip_while(|byte| *byte == 0)
        .collect();
    rlp_element(&bytes)
}

/// `serializeFederation`: its creation time in milliseconds, its creation
/// block, and its members sorted, each `FederationMember.serialize` (its BTC,
/// RSK and MST keys, here one key for all three) as an element.
pub(super) fn serialize_federation(keys: &[[u8; 33]], time_millis: u64, block: u64) -> Vec<u8> {
    let mut sorted = keys.to_vec();
    sorted.sort();
    let members: Vec<Vec<u8>> = sorted
        .iter()
        .map(|key| {
            rlp_element(&rlp_list(&[
                rlp_element(key),
                rlp_element(key),
                rlp_element(key),
            ]))
        })
        .collect();
    rlp_list(&[
        rlp_number(time_millis),
        rlp_number(block),
        rlp_list(&members),
    ])
}

/// `serializeFederationOnlyBtcKeys`: as [`serialize_federation`], each member
/// its BTC key alone.
pub(super) fn serialize_federation_only_btc_keys(
    keys: &[[u8; 33]],
    time_millis: u64,
    block: u64,
) -> Vec<u8> {
    let mut sorted = keys.to_vec();
    sorted.sort();
    let members: Vec<Vec<u8>> = sorted.iter().map(|key| rlp_element(key)).collect();
    rlp_list(&[
        rlp_number(time_millis),
        rlp_number(block),
        rlp_list(&members),
    ])
}

/// `serializeScript`: a list holding the script.
pub(super) fn serialize_script(script: &[u8]) -> Vec<u8> {
    rlp_list(&[rlp_element(script)])
}

/// Where a federation is kept: its cell and its format version's.
pub(super) type Slot = (&'static str, &'static str);

/// The federation that is, or is to be, active.
pub(super) const NEW: Slot = (
    storage::NEW_FEDERATION_KEY,
    storage::NEW_FEDERATION_FORMAT_VERSION_KEY,
);

/// The federation before it.
pub(super) const OLD: Slot = (
    storage::OLD_FEDERATION_KEY,
    storage::OLD_FEDERATION_FORMAT_VERSION_KEY,
);

/// The federation proposed from RSKIP419.
pub(super) const PROPOSED: Slot = (
    storage::PROPOSED_FEDERATION_KEY,
    storage::PROPOSED_FEDERATION_FORMAT_VERSION_KEY,
);

/// Keeps a federation of `keys` in `slot` as rskj does from RSKIP123, with
/// its format version, or, with no format, as it did before (its BTC keys
/// alone and no format cell).
pub(super) fn store_federation(
    ctx: &mut Ctx,
    slot: Slot,
    keys: &[[u8; 33]],
    time_millis: u64,
    block: u64,
    format: Option<u64>,
) {
    match format {
        Some(format) => {
            put(ctx, slot.0, &serialize_federation(keys, time_millis, block));
            put(ctx, slot.1, &rlp_number(format));
        }
        None => put(
            ctx,
            slot.0,
            &serialize_federation_only_btc_keys(keys, time_millis, block),
        ),
    }
}

// ---------------------------------------------------------------------------
// Votes
// ---------------------------------------------------------------------------

/// As many of the network's federation-change authorizers as decide a vote,
/// a majority (`FederationConstants.federationChangeAuthorizer`).
pub(super) fn deciding_authorizers(config: &BridgeConstants) -> Vec<RskAddress> {
    let all: Vec<RskAddress> = config
        .federation_change_authorizer_keys
        .iter()
        .map(|key| rsk_address_of(&hex::decode(key).expect("a key in hex")))
        .collect();
    all[..all.len() / 2 + 1].to_vec()
}

/// The context of a transaction `sender` sends.
fn sent_by(sender: RskAddress) -> BridgeTxContext {
    BridgeTxContext {
        rsk_sender: sender,
        ..BridgeTxContext::default()
    }
}

/// An `int256` answer, which here always fits an `i128`.
pub(super) fn int_answer(out: &[u8]) -> i128 {
    assert_eq!(out.len(), 32, "an int256 answer");
    let (high, low) = out.split_at(16);
    let low = i128::from_be_bytes(low.try_into().expect("16 bytes"));
    let fill = if low < 0 { 0xff } else { 0x00 };
    assert!(high.iter().all(|&b| b == fill), "an int256 past an i128");
    low
}

type Answer = Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError>;

/// Each deciding authorizer's vote through `vote`: their answers, `None`
/// where a vote throws.
fn votes(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    mut vote: impl FnMut(&mut Ctx, &BridgeTxContext) -> Answer,
) -> Vec<Option<i128>> {
    deciding_authorizers(config)
        .into_iter()
        .map(|sender| {
            vote(ctx, &sent_by(sender))
                .ok()
                .map(|out| int_answer(&out.bytes))
        })
        .collect()
}

/// The votes to create a pending federation and add `keys` to it, each its
/// member's BTC, RSK and MST key from RSKIP123 and its BTC key before it.
/// Every vote answers 1.
pub(super) fn pend_federation(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    keys: &[[u8; 33]],
) {
    let all_one = vec![Some(1); deciding_authorizers(config).len()];
    let created = votes(ctx, config, |ctx, tx_ctx| {
        governance::create_federation(ctx, 0, config, cfg, tx_ctx)
    });
    assert_eq!(created, all_one, "createFederation");
    let multikey = cfg.has_rskip123(ctx.block.number.to::<u64>());
    for key in keys {
        let added = if multikey {
            let args = encode::args(&[Arg::Bytes(key), Arg::Bytes(key), Arg::Bytes(key)]);
            votes(ctx, config, |ctx, tx_ctx| {
                governance::add_federator_public_key_multikey(ctx, &args, 0, config, cfg, tx_ctx)
            })
        } else {
            let args = encode::args(&[Arg::Bytes(key)]);
            votes(ctx, config, |ctx, tx_ctx| {
                governance::add_federator_public_key(ctx, &args, 0, config, cfg, tx_ctx)
            })
        };
        assert_eq!(added, all_one, "addFederatorPublicKey");
    }
}

/// The hash `getPendingFederationHash` answers.
pub(super) fn pending_hash(ctx: &mut Ctx) -> Vec<u8> {
    let out =
        getters::get_pending_federation_hash(ctx, 0).expect("getPendingFederationHash answers");
    let length = U256::from_be_slice(&out.bytes[32..64]).to::<usize>();
    out.bytes[64..64 + length].to_vec()
}

/// [`pend_federation`], then each deciding authorizer's vote to commit it:
/// their answers, `None` where a vote throws. The logs of the votes before
/// the commit are dropped, and the commit's kept.
pub(super) fn elect(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    keys: &[[u8; 33]],
) -> Vec<Option<i128>> {
    pend_federation(ctx, config, cfg, keys);
    let _ = ctx.journal_mut().take_logs();
    let args = encode::args(&[Arg::Bytes(&pending_hash(ctx))]);
    votes(ctx, config, |ctx, tx_ctx| {
        governance::commit_federation(ctx, &args, 0, config, cfg, tx_ctx)
    })
}

/// The answers of a commit every deciding authorizer's vote takes.
pub(super) fn committed(config: &BridgeConstants) -> Vec<Option<i128>> {
    vec![Some(1); deciding_authorizers(config).len()]
}

/// An event as the Bridge logs it: its topics, then its data.
pub(super) type Event = (Vec<B256>, Vec<u8>);

/// The events logged since the last taken.
pub(super) fn logs(ctx: &mut Ctx) -> Vec<Event> {
    ctx.journal_mut()
        .take_logs()
        .into_iter()
        .map(|log| (log.data.topics().to_vec(), log.data.data.to_vec()))
        .collect()
}

/// `commit_federation(bytes oldFederationBtcPublicKeys, string
/// oldFederationBtcAddress, bytes newFederationBtcPublicKeys, string
/// newFederationBtcAddress, int256 activationHeight)`, none of it indexed
/// (`BridgeEventLoggerImpl.logCommitFederation`).
pub(super) fn commit_federation_event(
    old: (&[[u8; 33]], &str),
    new: (&[[u8; 33]], &str),
    activation_height: u64,
) -> Event {
    let flat = |keys: &[[u8; 33]]| keys.concat();
    let (old_keys, new_keys) = (flat(old.0), flat(new.0));
    let data = encode::args(&[
        Arg::Bytes(&old_keys),
        Arg::Bytes(old.1.as_bytes()),
        Arg::Bytes(&new_keys),
        Arg::Bytes(new.1.as_bytes()),
        Arg::Word(encode::int(activation_height as i64)),
    ]);
    let topic = alloy_primitives::keccak256("commit_federation(bytes,string,bytes,string,int256)");
    (vec![topic], data)
}
