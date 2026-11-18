//! rskj's `BridgeTest`: the Bridge's methods called through `Bridge.execute`,
//! each where it opens and where it closes, each by every kind of call at
//! every upgrade from orchid to lovell700 (`msgTypesAndActivations`), and the
//! union bridge's (`UnionBridgeTest`).
//!
//! rskj's tests give the Bridge a mocked `BridgeSupport`, so that a method
//! that runs is one the mock saw called. Here every method is the real one,
//! and each test gives it what it needs to run to an answer, so that a
//! method that runs is one that answers rather than throws.

use super::support::*;
use alloy_primitives::{Address, U256};
use RskNetworkUpgrade::{
    Arrowhead600, Arrowhead631, Fingerroot500, Hop400, Iris300, Lovell700, Orchid, Papyrus200,
    Reed800, Vetiver900, Wasabi100,
};

const PUBLIC_KEY: &str = "039a060badbeb24bee49eb2063f616c0f0f0765d4ca646b20a88ce828f259fcdb9";
const SIGNATURE: &str = "3045022100a0963cea7551eb3174a3470c6ed25cda901c4b1093818d4d54792b87508820220220325f93b5aecc98385a664328e68d1cec7a2a2fe81810a7692358bd870aeecb74";
/// A mainnet P2PKH address, as the whitelist tests name it.
const BTC_ADDRESS: &str = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";

fn hex(s: &str) -> Vec<u8> {
    alloy_primitives::hex::decode(s).unwrap()
}

/// rskj's `BitcoinTestUtils.createHash(n)`: n's four bytes, low first, then
/// zeros.
fn btc_hash(n: u32) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[..4].copy_from_slice(&n.to_le_bytes());
    hash
}

/// rskj's `PegTestUtils.createHash3(n)`: n's two bytes, low first, then zeros.
fn rsk_hash(n: u16) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[..2].copy_from_slice(&n.to_le_bytes());
    hash
}

/// A Bitcoin block header as bitcoinj serializes it, its hashes given in the
/// order `Sha256Hash` holds them.
fn btc_header(prev: [u8; 32], merkle_root: [u8; 32], time: u32, bits: u32, nonce: u32) -> Vec<u8> {
    let mut header = 1u32.to_le_bytes().to_vec();
    header.extend(prev.iter().rev());
    header.extend(merkle_root.iter().rev());
    header.extend(time.to_le_bytes());
    header.extend(bits.to_le_bytes());
    header.extend(nonce.to_le_bytes());
    header
}

/// A partial merkle tree of a block of one transaction, `leaf`, which it
/// matches.
fn one_leaf_tree(leaf: [u8; 32]) -> Vec<u8> {
    let mut tree = 1u32.to_le_bytes().to_vec();
    tree.push(1);
    tree.extend_from_slice(&leaf);
    tree.extend_from_slice(&[1, 0x01]);
    tree
}

/// A `uint256` argument too big for an `i64`.
fn word(value: U256) -> Arg<'static> {
    Arg::Word(value.to_be_bytes())
}

fn one_rbtc() -> U256 {
    U256::from(10u64).pow(U256::from(18))
}

// ---- every kind of call at every upgrade (`msgTypesAndActivations`) ----

/// Where rskj's test has a method open.
#[derive(Clone, Copy)]
enum Open {
    Always,
    /// From this upgrade on.
    Since(RskNetworkUpgrade),
    /// Before this upgrade only.
    Until(RskNetworkUpgrade),
}

impl Open {
    fn at(self, upgrade: RskNetworkUpgrade) -> bool {
        match self {
            Open::Always => true,
            Open::Since(first) => upgrade >= first,
            Open::Until(closed) => upgrade < closed,
        }
    }
}

/// The kinds of call a method takes from RSKIP417 (arrowhead600) on: rskj's
/// `RESTRICTED_TO_CALL` and `ALLOW_STATIC_CALL`. Before it, every kind.
#[derive(Clone, Copy, PartialEq)]
enum Takes {
    Call,
    CallOrStaticCall,
}

const MATRIX_UPGRADES: [RskNetworkUpgrade; 8] = [
    Orchid,
    Wasabi100,
    Papyrus200,
    Iris300,
    Hop400,
    Fingerroot500,
    Arrowhead600,
    Lovell700,
];

/// The `MsgType`s an opcode makes (`MsgType.fromOpcode`). rskj's fifth, POST,
/// no opcode makes, and nothing here stands for it.
const KINDS: [(&str, BridgeCallKind); 4] = [
    ("CALL", BridgeCallKind::Call),
    ("CALLCODE", BridgeCallKind::DelegateOrCallCode),
    ("DELEGATECALL", BridgeCallKind::DelegateOrCallCode),
    ("STATICCALL", BridgeCallKind::StaticCall),
];

/// One of rskj's tests over `msgTypesAndActivations`: at each upgrade, by each
/// kind of call, the method throws where it is closed and, from arrowhead600,
/// where it doesn't take that kind; everywhere else it runs, and `answered`
/// looks at its answer.
fn by_every_kind_at_every_upgrade(
    data: &[u8],
    open: Open,
    takes: Takes,
    prepare: impl Fn(&mut Bridge, RskNetworkUpgrade),
    answered: impl Fn(&[u8]),
) {
    for upgrade in MATRIX_UPGRADES {
        for (name, kind) in KINDS {
            let mut bridge = Bridge::mainnet_up_to(upgrade);
            bridge.kind = kind;
            prepare(&mut bridge, upgrade);
            let result = bridge.execute(data);
            let taken = kind == BridgeCallKind::Call
                || (takes == Takes::CallOrStaticCall && kind == BridgeCallKind::StaticCall);
            if !open.at(upgrade) || (upgrade >= Arrowhead600 && !taken) {
                assert!(
                    threw(&result),
                    "{name} at {upgrade:?} must throw: {result:?}"
                );
            } else {
                match result {
                    Ok(output) => answered(&output.bytes),
                    Err(e) => panic!("{name} at {upgrade:?} must run: {e:?}"),
                }
            }
        }
    }
}

/// A transaction of a stranger's, as rskj's `BridgeBuilder` gives one.
fn as_is(_: &mut Bridge, _: RskNetworkUpgrade) {}

/// rskj's `isLocalCallTransaction()` true, which a local-only getter needs.
fn locally(bridge: &mut Bridge, _: RskNetworkUpgrade) {
    bridge.local_call = true;
}

/// From a member of the active federation, which the genesis one is here.
fn from_a_federator(bridge: &mut Bridge, _: RskNetworkUpgrade) {
    bridge.caller = genesis_federator(&bridge.constants);
}

/// A method with nothing to answer, to a transaction's own call.
fn nothing(answer: &[u8]) {
    assert!(answer.is_empty(), "{answer:?}");
}

/// One word: a number, a flag or an address.
fn one_word(answer: &[u8]) {
    assert_eq!(answer.len(), 32, "{answer:?}");
}

/// Bytes or a string: where they are, how long, then what they are.
fn dynamic(answer: &[u8]) {
    assert!(
        answer.len() >= 64 && answer.len().is_multiple_of(32),
        "{answer:?}"
    );
}

/// A vote or a change of the whitelist from no authorizer: `-10`.
fn unauthorized(answer: &[u8]) {
    assert_eq!(int_of(answer), -10);
}

#[test]
fn add_federator_public_key_closes_at_wasabi100() {
    let key = hex(PUBLIC_KEY);
    let data = call_data("addFederatorPublicKey(bytes)", &[Arg::Bytes(&key)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Until(Wasabi100),
        Takes::Call,
        as_is,
        unauthorized,
    );
}

#[test]
fn add_federator_public_key_multikey_opens_at_wasabi100() {
    let key = hex(PUBLIC_KEY);
    let data = call_data(
        "addFederatorPublicKeyMultikey(bytes,bytes,bytes)",
        &[Arg::Bytes(&key), Arg::Bytes(&key), Arg::Bytes(&key)],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Wasabi100),
        Takes::Call,
        as_is,
        unauthorized,
    );
}

#[test]
fn add_lock_whitelist_address_closes_at_orchid() {
    let data = call_data(
        "addLockWhitelistAddress(string,int256)",
        &[Arg::Bytes(BTC_ADDRESS.as_bytes()), int(100_000)],
    );
    by_every_kind_at_every_upgrade(&data, Open::Until(Orchid), Takes::Call, as_is, unauthorized);
}

#[test]
fn add_one_off_lock_whitelist_address_opens_at_orchid() {
    let data = call_data(
        "addOneOffLockWhitelistAddress(string,int256)",
        &[Arg::Bytes(BTC_ADDRESS.as_bytes()), int(100_000)],
    );
    by_every_kind_at_every_upgrade(&data, Open::Since(Orchid), Takes::Call, as_is, unauthorized);
}

#[test]
fn add_unlimited_lock_whitelist_address_opens_at_orchid() {
    let data = call_data(
        "addUnlimitedLockWhitelistAddress(string)",
        &[Arg::Bytes(BTC_ADDRESS.as_bytes())],
    );
    by_every_kind_at_every_upgrade(&data, Open::Since(Orchid), Takes::Call, as_is, unauthorized);
}

#[test]
fn add_signature_takes_only_a_call_from_arrowhead600() {
    let (key, signature) = (hex(PUBLIC_KEY), hex(SIGNATURE));
    let data = call_data(
        "addSignature(bytes,bytes[],bytes)",
        &[
            Arg::Bytes(&key),
            Arg::BytesArray(&[&signature]),
            Arg::Bytes(&rsk_hash(1)),
        ],
    );
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, from_a_federator, nothing);
}

#[test]
fn commit_federation_takes_only_a_call_from_arrowhead600() {
    let data = call_data("commitFederation(bytes)", &[Arg::Bytes(&rsk_hash(2))]);
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

#[test]
fn create_federation_takes_only_a_call_from_arrowhead600() {
    let data = selector_only("createFederation()");
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

#[test]
fn btc_blockchain_best_chain_height_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getBtcBlockchainBestChainHeight()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn btc_blockchain_initial_block_height_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getBtcBlockchainInitialBlockHeight()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn btc_blockchain_block_locator_closes_at_orchid() {
    let data = selector_only("getBtcBlockchainBlockLocator()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Until(Orchid),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

/// rskj's test asks for depth 1000 of a mocked chain. The chain here is the
/// one the Bridge starts from, its head alone, so it asks for depth 0.
#[test]
fn btc_blockchain_block_hash_at_depth_opens_at_orchid() {
    let data = call_data("getBtcBlockchainBlockHashAtDepth(int256)", &[int(0)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Orchid),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn btc_transaction_confirmations_open_at_wasabi100() {
    let branch = [btc_hash(10), btc_hash(11), btc_hash(12)];
    let data = call_data(
        "getBtcTransactionConfirmations(bytes32,bytes32,uint256,bytes32[])",
        &[
            Arg::Word(btc_hash(1)),
            Arg::Word(btc_hash(2)),
            int(1),
            Arg::Words(&branch),
        ],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Wasabi100),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

#[test]
fn btc_tx_hash_processed_height_takes_a_call_or_staticcall_from_arrowhead600() {
    let hash = alloy_primitives::hex::encode(btc_hash(1));
    let data = call_data(
        "getBtcTxHashProcessedHeight(string)",
        &[Arg::Bytes(hash.as_bytes())],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn federation_address_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFederationAddress()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn federation_creation_block_number_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFederationCreationBlockNumber()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn federation_creation_time_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFederationCreationTime()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn federation_size_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFederationSize()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn federation_threshold_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFederationThreshold()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn federator_public_key_closes_at_wasabi100() {
    let data = call_data("getFederatorPublicKey(int256)", &[int(1)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Until(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn federator_public_key_of_type_opens_at_wasabi100() {
    let data = call_data(
        "getFederatorPublicKeyOfType(int256,string)",
        &[int(1), Arg::Bytes(b"btc")],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn fee_per_kb_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getFeePerKb()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn lock_whitelist_address_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = call_data("getLockWhitelistAddress(int256)", &[int(1)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn lock_whitelist_entry_by_address_opens_at_orchid() {
    let data = call_data(
        "getLockWhitelistEntryByAddress(string)",
        &[Arg::Bytes(BTC_ADDRESS.as_bytes())],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Orchid),
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn lock_whitelist_size_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getLockWhitelistSize()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn minimum_lock_tx_value_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getMinimumLockTxValue()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn pending_federation_hash_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getPendingFederationHash()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn pending_federation_size_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getPendingFederationSize()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn pending_federator_public_key_closes_at_wasabi100() {
    let data = call_data("getPendingFederatorPublicKey(int256)", &[int(1)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Until(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn pending_federator_public_key_of_type_opens_at_wasabi100() {
    let data = call_data(
        "getPendingFederatorPublicKeyOfType(int256,string)",
        &[int(1), Arg::Bytes(b"btc")],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn retiring_federation_address_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getRetiringFederationAddress()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn retiring_federation_creation_block_number_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getRetiringFederationCreationBlockNumber()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn retiring_federation_creation_time_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getRetiringFederationCreationTime()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn retiring_federation_size_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getRetiringFederationSize()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn retiring_federation_threshold_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getRetiringFederationThreshold()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn retiring_federator_public_key_closes_at_wasabi100() {
    let data = call_data("getRetiringFederatorPublicKey(int256)", &[int(1)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Until(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn retiring_federator_public_key_of_type_opens_at_wasabi100() {
    let data = call_data(
        "getRetiringFederatorPublicKeyOfType(int256,string)",
        &[int(1), Arg::Bytes(b"btc")],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Wasabi100),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn proposed_federation_address_opens_at_lovell700() {
    let data = selector_only("getProposedFederationAddress()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn proposed_federation_size_opens_at_lovell700() {
    let data = selector_only("getProposedFederationSize()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn proposed_federation_creation_time_opens_at_lovell700() {
    let data = selector_only("getProposedFederationCreationTime()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn proposed_federation_creation_block_number_opens_at_lovell700() {
    let data = selector_only("getProposedFederationCreationBlockNumber()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn proposed_federator_public_key_of_type_opens_at_lovell700() {
    let data = call_data(
        "getProposedFederatorPublicKeyOfType(int256,string)",
        &[int(1), Arg::Bytes(b"btc")],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn state_for_btc_release_client_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getStateForBtcReleaseClient()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn state_for_svp_client_opens_at_lovell700() {
    let data = selector_only("getStateForSvpClient()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Lovell700),
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn state_for_debugging_takes_a_call_or_staticcall_from_arrowhead600() {
    let data = selector_only("getStateForDebugging()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        dynamic,
    );
}

#[test]
fn locking_cap_opens_at_papyrus200() {
    let data = selector_only("getLockingCap()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Papyrus200),
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

#[test]
fn active_powpeg_redeem_script_opens_at_hop400() {
    let data = selector_only("getActivePowpegRedeemScript()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Hop400),
        Takes::CallOrStaticCall,
        as_is,
        dynamic,
    );
}

#[test]
fn active_federation_creation_block_height_opens_at_iris300() {
    let data = selector_only("getActiveFederationCreationBlockHeight()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Iris300),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

/// From a stranger, so the cap stays as it is and the answer is false.
#[test]
fn increase_locking_cap_opens_at_papyrus200() {
    let data = call_data("increaseLockingCap(int256)", &[int(1)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Papyrus200),
        Takes::Call,
        as_is,
        |answer| assert!(!bool_of(answer)),
    );
}

#[test]
fn btc_tx_hash_already_processed_takes_a_call_or_staticcall_from_arrowhead600() {
    use sha2::Digest;
    let hash = alloy_primitives::hex::encode(sha2::Sha256::digest(b"btcTxHash"));
    let data = call_data(
        "isBtcTxHashAlreadyProcessed(string)",
        &[Arg::Bytes(hash.as_bytes())],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Always,
        Takes::CallOrStaticCall,
        locally,
        one_word,
    );
}

/// From a federator before wasabi100 and from iris300 on, where only the
/// federations may send headers; from anyone in between.
#[test]
fn receive_headers_takes_only_a_call_from_arrowhead600() {
    let data = selector_only("receiveHeaders(bytes[])");
    let prepare = |bridge: &mut Bridge, upgrade| {
        if !(Wasabi100..Iris300).contains(&upgrade) {
            bridge.caller = genesis_federator(&bridge.constants);
        }
    };
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, prepare, nothing);
}

#[test]
fn receive_header_opens_at_iris300() {
    let header = btc_header(btc_hash(1), btc_hash(2), 1, 100, 1);
    let data = call_data("receiveHeader(bytes)", &[Arg::Bytes(&header)]);
    by_every_kind_at_every_upgrade(&data, Open::Since(Iris300), Takes::Call, as_is, one_word);
}

/// rskj's test registers `[1]` at height 0 with the tree `[2]` into a mock.
/// The real method throws for a tree of the wrong size once the transaction
/// has its confirmations, so the height here is past the chain's head, which
/// the method lets go without registering anything.
#[test]
fn register_btc_transaction_takes_only_a_call_from_arrowhead600() {
    let data = call_data(
        "registerBtcTransaction(bytes,int256,bytes)",
        &[Arg::Bytes(&[1]), int(i64::from(i32::MAX)), Arg::Bytes(&[2])],
    );
    let prepare = |bridge: &mut Bridge, upgrade| {
        if upgrade < Iris300 {
            bridge.caller = genesis_federator(&bridge.constants);
        }
    };
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, prepare, nothing);
}

#[test]
fn release_btc_takes_only_a_call_from_arrowhead600() {
    let data = selector_only("releaseBtc()");
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, nothing);
}

#[test]
fn remove_lock_whitelist_address_takes_only_a_call_from_arrowhead600() {
    let data = call_data(
        "removeLockWhitelistAddress(string)",
        &[Arg::Bytes(BTC_ADDRESS.as_bytes())],
    );
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

#[test]
fn rollback_federation_takes_only_a_call_from_arrowhead600() {
    let data = selector_only("rollbackFederation()");
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

#[test]
fn set_lock_whitelist_disable_block_delay_takes_only_a_call_from_arrowhead600() {
    let data = call_data("setLockWhitelistDisableBlockDelay(int256)", &[int(100)]);
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

#[test]
fn update_collections_takes_only_a_call_from_arrowhead600() {
    let data = selector_only("updateCollections()");
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, from_a_federator, nothing);
}

#[test]
fn vote_fee_per_kb_takes_only_a_call_from_arrowhead600() {
    let data = call_data("voteFeePerKbChange(int256)", &[int(10_000)]);
    by_every_kind_at_every_upgrade(&data, Open::Always, Takes::Call, as_is, unauthorized);
}

/// rskj's test hands a mock an empty transaction and an empty tree. The real
/// method throws for a tree of the wrong size, so the tree here is a whole
/// one that doesn't hold the transaction, which the method lets go.
#[test]
fn register_btc_coinbase_transaction_opens_at_papyrus200() {
    let tree = one_leaf_tree([0x11; 32]);
    let data = call_data(
        "registerBtcCoinbaseTransaction(bytes,bytes32,bytes,bytes32,bytes32)",
        &[
            Arg::Bytes(&[0u8; 32]),
            Arg::Word(btc_hash(1)),
            Arg::Bytes(&tree),
            Arg::Word(btc_hash(2)),
            Arg::Word([0; 32]),
        ],
    );
    by_every_kind_at_every_upgrade(&data, Open::Since(Papyrus200), Takes::Call, as_is, nothing);
}

#[test]
fn btc_block_coinbase_transaction_information_opens_at_papyrus200() {
    let data = call_data(
        "hasBtcBlockCoinbaseTransactionInformation(bytes32)",
        &[Arg::Word(btc_hash(2))],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Papyrus200),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

/// A transaction's own call, which the Flyover's checks turn down for not
/// coming from a contract (`UNPROCESSABLE_TX_NOT_CONTRACT_ERROR`, -300).
#[test]
fn register_fast_bridge_btc_transaction_opens_at_iris300() {
    let lbc = Address::from_slice(&hex("FcE93641243D1EFB6131277cCD1c0a60460d5610"));
    let data = call_data(
        "registerFastBridgeBtcTransaction(bytes,uint256,bytes,bytes32,bytes,address,bytes,bool)",
        &[
            Arg::Bytes(&[1]),
            int(1),
            Arg::Bytes(&[2]),
            Arg::Word(rsk_hash(2)),
            Arg::Bytes(&[0u8; 21]),
            address(lbc),
            Arg::Bytes(&[0u8; 21]),
            boolean(true),
        ],
    );
    by_every_kind_at_every_upgrade(&data, Open::Since(Iris300), Takes::Call, as_is, |answer| {
        assert_eq!(int_of(answer), -300)
    });
}

#[test]
fn btc_blockchain_best_block_header_opens_at_iris300() {
    let data = selector_only("getBtcBlockchainBestBlockHeader()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Iris300),
        Takes::CallOrStaticCall,
        as_is,
        dynamic,
    );
}

#[test]
fn btc_blockchain_block_header_by_hash_opens_at_iris300() {
    let data = call_data(
        "getBtcBlockchainBlockHeaderByHash(bytes32)",
        &[Arg::Word([0; 32])],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Iris300),
        Takes::CallOrStaticCall,
        as_is,
        dynamic,
    );
}

#[test]
fn btc_blockchain_block_header_by_height_opens_at_iris300() {
    let data = call_data("getBtcBlockchainBlockHeaderByHeight(uint256)", &[int(20)]);
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Iris300),
        Takes::CallOrStaticCall,
        as_is,
        dynamic,
    );
}

#[test]
fn btc_blockchain_parent_block_header_by_hash_opens_at_iris300() {
    let data = call_data(
        "getBtcBlockchainParentBlockHeaderByHash(bytes32)",
        &[Arg::Word([0; 32])],
    );
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Iris300),
        Takes::CallOrStaticCall,
        as_is,
        dynamic,
    );
}

#[test]
fn next_pegout_creation_block_number_opens_at_hop400() {
    let data = selector_only("getNextPegoutCreationBlockNumber()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Hop400),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

#[test]
fn queued_pegouts_count_opens_at_hop400() {
    let data = selector_only("getQueuedPegoutsCount()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Hop400),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

#[test]
fn estimated_fees_for_next_pegout_event_open_at_hop400() {
    let data = selector_only("getEstimatedFeesForNextPegOutEvent()");
    by_every_kind_at_every_upgrade(
        &data,
        Open::Since(Hop400),
        Takes::CallOrStaticCall,
        as_is,
        one_word,
    );
}

/// rskj's `assertVoidMethodResult`, which its tests apply to a transaction's
/// own call, where it can't be seen: a method with nothing to answer answers
/// null before RSKIP417 (arrowhead600) and an empty array from it. A
/// transaction's call ends the same either way; a contract's call to
/// `updateCollections` fails on the null from RSKIP197 (iris300), as
/// `Program.executePrecompiledAndHandleError` writes it, and gets the empty
/// array from arrowhead600.
#[test]
fn a_method_with_nothing_to_answer_answers_a_contract_from_arrowhead600() {
    let data = selector_only("updateCollections()");
    for (upgrade, answers) in [
        (Iris300, false),
        (Fingerroot500, false),
        (Arrowhead600, true),
    ] {
        let mut bridge = Bridge::mainnet_up_to(upgrade);
        bridge.caller = genesis_federator(&bridge.constants);
        bridge.depth = 2;
        let result = bridge.execute(&data);
        if answers {
            assert!(result.unwrap().bytes.is_empty());
        } else {
            assert!(threw(&result), "{upgrade:?}: {result:?}");
        }
    }
}

// ---- where each method opens and closes ----

#[test]
fn the_active_powpeg_redeem_script_is_the_genesis_federations_from_hop400() {
    let data = selector_only("getActivePowpegRedeemScript()");
    assert!(threw(&Bridge::mainnet_up_to(Iris300).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Hop400);
    let script = bytes_of(&bridge.answer(&data));
    // The genesis federation's standard multisig, built here from its keys:
    // OP_m, each key sorted, OP_n, OP_CHECKMULTISIG.
    let keys = crate::bridge::peg::genesis_federation_keys(&bridge.constants);
    let (n, m) = (keys.len() as u8, keys.len() as u8 / 2 + 1);
    let mut expected = vec![0x50 + m];
    for key in &keys {
        expected.push(33);
        expected.extend_from_slice(key);
    }
    expected.extend_from_slice(&[0x50 + n, 0xae]);
    assert_eq!(script, expected);
}

#[test]
fn the_locking_cap_is_its_initial_value_from_papyrus200() {
    let data = selector_only("getLockingCap()");
    assert!(threw(&Bridge::mainnet_up_to(Wasabi100).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Papyrus200);
    bridge.local_call = true;
    let cap = int_of(&bridge.answer(&data));
    assert_eq!(cap, bridge.constants.initial_locking_cap as i64);
}

#[test]
fn increase_locking_cap_is_closed_before_papyrus200() {
    let data = selector_only("increaseLockingCap(int256)");
    assert!(threw(&Bridge::mainnet_up_to(Wasabi100).execute(&data)));
}

/// rskj's test has a mock answer true for 1 and for 210,000,000 from anyone.
/// The real method answers false to a caller who is no authorizer, and to
/// its authorizer true for a cap from the current one up to the current one
/// times the multiplier, and false on either side of that.
#[test]
fn increase_locking_cap_raises_the_cap_up_to_its_multiple_for_its_authorizer() {
    let cap_data = |cap: u64| call_data("increaseLockingCap(int256)", &[int(cap as i64)]);
    let mut bridge = Bridge::mainnet_up_to(Vetiver900);
    for value in [1, 210_000_000] {
        assert!(!bool_of(&bridge.answer(&cap_data(value))));
    }

    bridge.caller = address_of_key(bridge.constants.increase_locking_cap_authorizer_keys[0]);
    let current = bridge.constants.initial_locking_cap;
    let multiplier = bridge.constants.locking_cap_increments_multiplier;
    assert!(!bool_of(&bridge.answer(&cap_data(current - 1))));
    assert!(!bool_of(
        &bridge.answer(&cap_data(current * multiplier + 1))
    ));
    assert!(bool_of(&bridge.answer(&cap_data(current * multiplier))));
    // No more than the cap is no increase, and no less is none either.
    assert!(bool_of(&bridge.answer(&cap_data(current * multiplier))));

    bridge.local_call = true;
    let cap = int_of(&bridge.answer(&selector_only("getLockingCap()")));
    assert_eq!(cap, (current * multiplier) as i64);
}

/// What rskj's four tests of an invalid cap send: a byte where a word goes, a
/// cap past a `long`, and, without the method's selector as rskj's
/// `encodeArguments` writes them, -1 and 0, which no method's selector
/// starts. The real method throws for -1 and 0 with its selector too, before
/// it asks who calls.
#[test]
fn increase_locking_cap_throws_for_a_cap_that_is_no_positive_long() {
    let mut bridge = Bridge::mainnet_up_to(Vetiver900);
    let selector = selector_only("increaseLockingCap(int256)");
    let past_a_long = hex("0000000000000000000000000000000000000000000000080000000000000000");
    for arguments in [hex("ab"), past_a_long] {
        let data = [selector.clone(), arguments].concat();
        assert!(threw(&bridge.execute(&data)));
    }
    for cap in [-1, 0] {
        assert!(threw(&bridge.execute(&encode::int(cap))));
        let data = call_data("increaseLockingCap(int256)", &[int(cap)]);
        assert!(threw(&bridge.execute(&data)));
    }
}

#[test]
fn register_btc_coinbase_transaction_is_closed_before_papyrus200() {
    let data = call_data(
        "registerBtcCoinbaseTransaction(bytes,bytes32,bytes,bytes32,bytes32)",
        &[
            Arg::Bytes(&[0; 32]),
            int(0),
            Arg::Bytes(&[0; 32]),
            int(0),
            int(0),
        ],
    );
    assert!(threw(&Bridge::mainnet_up_to(Wasabi100).execute(&data)));
}

/// rskj's test hands a mock a 32-byte tree. The real method throws for a
/// tree of that size, so the tree here is a whole one that doesn't hold
/// the transaction, which the method lets go.
#[test]
fn register_btc_coinbase_transaction_runs_from_papyrus200() {
    let tree = one_leaf_tree([0x11; 32]);
    let data = call_data(
        "registerBtcCoinbaseTransaction(bytes,bytes32,bytes,bytes32,bytes32)",
        &[
            Arg::Bytes(&[0; 32]),
            int(0),
            Arg::Bytes(&tree),
            int(0),
            int(0),
        ],
    );
    let mut bridge = Bridge::mainnet_up_to(Papyrus200);
    assert!(bridge.answer(&data).is_empty());
}

#[test]
fn register_btc_coinbase_transaction_throws_for_arguments_that_do_not_decode() {
    let mut bridge = Bridge::mainnet_up_to(Papyrus200);
    let selector =
        selector_only("registerBtcCoinbaseTransaction(bytes,bytes32,bytes,bytes32,bytes32)");
    let past_a_long = hex("0000000000000000000000000000000000000000000000080000000000000000");
    for arguments in [vec![], hex("ab"), past_a_long] {
        let data = [selector.clone(), arguments].concat();
        assert!(threw(&bridge.execute(&data)));
    }
}

fn register_zero_transaction() -> Vec<u8> {
    call_data(
        "registerBtcTransaction(bytes,int256,bytes)",
        &[
            Arg::Bytes(&[0; 32]),
            int(i64::from(i32::MAX)),
            Arg::Bytes(&[0; 32]),
        ],
    )
}

#[test]
fn register_btc_transaction_turns_a_stranger_down_before_iris300() {
    let mut bridge = Bridge::mainnet_up_to(Papyrus200);
    bridge.caller = Address::with_last_byte(1);
    assert!(threw(&bridge.execute(&register_zero_transaction())));
}

#[test]
fn register_btc_transaction_takes_a_federator_before_iris300() {
    let mut bridge = Bridge::mainnet_up_to(Papyrus200);
    bridge.caller = genesis_federator(&bridge.constants);
    assert!(bridge.answer(&register_zero_transaction()).is_empty());
}

#[test]
fn register_btc_transaction_takes_anyone_from_iris300() {
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    assert!(bridge.answer(&register_zero_transaction()).is_empty());
}

#[test]
fn the_active_federation_creation_block_height_is_read_from_iris300() {
    let data = selector_only("getActiveFederationCreationBlockHeight()");
    assert!(threw(&Bridge::mainnet_up_to(Papyrus200).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Iris300);
    crate::bridge::storage::bridge_store_u256(
        &mut bridge.ctx,
        crate::bridge::storage::ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        U256::from(1),
    );
    assert_eq!(int_of(&bridge.answer(&data)), 1);
}

/// rskj's `FederationSupportImplTest.getActiveFederationCreationBlockHeight_with…`:
/// from the block that is the activation age past the next federation's
/// creation block, the answer is that block, though no `updateCollections`
/// has moved it to the active federation's cell yet; before, the active
/// federation's, or 0 where neither is stored. On mainnet, at hop400's
/// activation age (18,500 blocks) and at fingerroot500's (40,320).
#[test]
fn the_active_federation_creation_block_height_is_the_next_ones_from_its_activation_age() {
    use crate::bridge::serialization::rlp_encode_u64;
    use crate::bridge::storage::{
        bridge_store_bytes_named, ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
    };
    let data = selector_only("getActiveFederationCreationBlockHeight()");
    let answer = |number: u64, next: Option<u64>, active: Option<u64>| {
        let mut bridge = Bridge::new(
            RskHardforkConfig::mainnet(),
            BridgeConstants::mainnet(),
            number,
        );
        for (key, height) in [
            (NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY, next),
            (ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY, active),
        ] {
            if let Some(height) = height {
                bridge_store_bytes_named(&mut bridge.ctx, key, &rlp_encode_u64(height));
            }
        }
        int_of(&bridge.answer(&data))
    };
    let next = 4_700_000;
    assert_eq!(answer(next + 18_499, Some(next), None), 0);
    assert_eq!(answer(next + 18_499, Some(next), Some(1)), 1);
    assert_eq!(answer(next + 18_500, Some(next), None), 4_700_000);
    assert_eq!(answer(next + 18_500, Some(next), Some(1)), 4_700_000);
    let next = 5_500_000;
    assert_eq!(answer(next + 18_500, Some(next), Some(1)), 1);
    assert_eq!(answer(next + 40_319, Some(next), None), 0);
    assert_eq!(answer(next + 40_320, Some(next), Some(1)), 5_500_000);
    assert_eq!(answer(next, None, None), 0);
    assert_eq!(answer(next, None, Some(1)), 1);
}

/// rskj's `FederationSupportImplTest.getActiveFederationCreationBlockHeight_beforeRSKIP186_returnsZero`:
/// before RSKIP-186 the federation support answers 0, whatever is stored. The
/// Bridge has the method only from RSKIP-186, so this asks the federation
/// support itself, on mainnet at block 3,000,000, before Iris.
#[test]
fn before_rskip186_the_active_federation_creation_block_height_is_zero() {
    use crate::bridge::serialization::rlp_encode_u64;
    use crate::bridge::storage::{
        bridge_store_bytes_named, ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
    };
    let mut bridge = Bridge::new(
        RskHardforkConfig::mainnet(),
        BridgeConstants::mainnet(),
        3_000_000,
    );
    for (key, height) in [
        (NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY, 1_000_000),
        (ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY, 1),
    ] {
        bridge_store_bytes_named(&mut bridge.ctx, key, &rlp_encode_u64(height));
    }
    let output = crate::bridge::governance::get_active_federation_creation_block_height(
        &mut bridge.ctx,
        3_000,
        &BridgeConstants::mainnet(),
        &RskHardforkConfig::mainnet(),
    )
    .expect("an answer");
    assert_eq!(output.bytes.as_ref(), &[0u8; 32][..]);
}

fn flyover_data(refund: &[u8], lbc: Address, lp: &[u8]) -> Vec<u8> {
    call_data(
        "registerFastBridgeBtcTransaction(bytes,uint256,bytes,bytes32,bytes,address,bytes,bool)",
        &[
            Arg::Bytes(&[0; 32]),
            int(1),
            Arg::Bytes(&[0; 32]),
            Arg::Word([0; 32]),
            Arg::Bytes(refund),
            address(lbc),
            Arg::Bytes(lp),
            boolean(true),
        ],
    )
}

#[test]
fn register_flyover_btc_transaction_is_closed_before_iris300() {
    let hash160 = [0x42; 20];
    let data = flyover_data(&hash160, Address::repeat_byte(0x2e), &hash160);
    assert!(threw(&Bridge::mainnet_up_to(Papyrus200).execute(&data)));
}

/// rskj's testnet P2SH refund address, `2MyEXHyt2fXqdFm3r4xXEkTdbwdZm7qFiDP`,
/// as `serializeBtcAddressWithVersion` writes it before RSKIP284: its
/// version 196 as `BigInteger.toByteArray()` writes it, two bytes, then its
/// hash; and from RSKIP284, one byte of version.
fn p2sh_refund_address(rskip284: bool) -> Vec<u8> {
    let decoded = bitcoin::base58::decode_check("2MyEXHyt2fXqdFm3r4xXEkTdbwdZm7qFiDP").unwrap();
    assert_eq!(decoded[0], 196);
    if rskip284 {
        decoded
    } else {
        [vec![0], decoded].concat()
    }
}

/// A testnet P2PKH address, version 111, for the liquidity provider.
fn p2pkh_lp_address() -> Vec<u8> {
    [vec![111], vec![0x24; 20]].concat()
}

/// On rskj's regtest (`Constants.regtest()`), at block 0 of `last`.
fn regtest_up_to(last: RskNetworkUpgrade) -> Bridge {
    Bridge::new(
        up_to(last, RSK_REGTEST_CHAIN_ID),
        BridgeConstants::rskj_regtest(),
        0,
    )
}

#[test]
fn a_p2sh_refund_address_is_a_generic_error_before_hop400() {
    let mut bridge = regtest_up_to(Iris300);
    let data = flyover_data(&p2sh_refund_address(false), STRANGER, &p2pkh_lp_address());
    assert_eq!(int_of(&bridge.answer(&data)), -900);
}

/// rskj's test has a mock answer 2 once the address gets through. The real
/// checks it reaches turn a transaction's own call down for not coming
/// from a contract.
#[test]
fn a_p2sh_refund_address_gets_through_from_hop400() {
    let mut bridge = regtest_up_to(Hop400);
    let data = flyover_data(&p2sh_refund_address(true), STRANGER, &p2pkh_lp_address());
    assert_eq!(int_of(&bridge.answer(&data)), -300);
}

/// A refund address of twenty bytes, a bare hash.
#[test]
fn a_refund_address_that_does_not_parse_is_a_generic_error() {
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    let data = flyover_data(&[0x42; 20], STRANGER, &[0x24; 20]);
    assert_eq!(int_of(&bridge.answer(&data)), -900);
}

#[test]
fn register_flyover_btc_transaction_throws_for_arguments_that_do_not_decode() {
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    let selector = selector_only(
        "registerFastBridgeBtcTransaction(bytes,uint256,bytes,bytes32,bytes,address,bytes,bool)",
    );
    assert!(threw(&bridge.execute(&selector)));
    assert!(threw(&bridge.execute(&[selector, vec![0; 32]].concat())));
}

#[test]
fn receive_header_is_closed_before_iris300() {
    let header = btc_header(btc_hash(1), btc_hash(1), 1, 0x207fffff, 1);
    let data = call_data("receiveHeader(bytes)", &[Arg::Bytes(&header)]);
    assert!(threw(&Bridge::mainnet_up_to(Papyrus200).execute(&data)));
}

/// rskj's test has a mock take the header, and its answer, 0, is the mock's.
/// The real method looks for the header's parent, which no chain here has:
/// `RECEIVE_HEADER_CANT_FOUND_PREVIOUS_BLOCK`, -3.
#[test]
fn receive_header_answers_for_a_header_with_no_parent_from_iris300() {
    let header = btc_header(btc_hash(1), btc_hash(2), 1, 100, 1);
    let data = call_data("receiveHeader(bytes)", &[Arg::Bytes(&header)]);
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    assert_eq!(int_of(&bridge.answer(&data)), -3);
}

#[test]
fn receive_header_throws_for_no_argument() {
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    assert!(threw(
        &bridge.execute(&selector_only("receiveHeader(bytes)"))
    ));
}

#[test]
fn receive_headers_turns_a_stranger_down_from_iris300() {
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    assert!(threw(
        &bridge.execute(&selector_only("receiveHeaders(bytes[])"))
    ));
}

#[test]
fn a_header_of_the_wrong_size_is_answered_minus_20() {
    let data = call_data("receiveHeader(bytes)", &[Arg::Bytes(&[0; 32])]);
    let mut bridge = Bridge::mainnet_up_to(Iris300);
    assert_eq!(int_of(&bridge.answer(&data)), -20);
}

#[test]
fn the_next_pegout_creation_block_number_is_read_from_hop400() {
    let data = selector_only("getNextPegoutCreationBlockNumber()");
    assert!(threw(&Bridge::mainnet_up_to(Iris300).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Hop400);
    crate::bridge::storage::bridge_store_u256(
        &mut bridge.ctx,
        crate::bridge::storage::NEXT_PEGOUT_HEIGHT_KEY,
        U256::from(1),
    );
    assert_eq!(int_of(&bridge.answer(&data)), 1);
}

/// rskj's test has a mock count one peg-out. Here a peg-out is asked for,
/// with `releaseBtc` and a bitcoin, which pays the fees a peg-out of the
/// genesis federation's needs, and then counted.
#[test]
fn the_queued_pegouts_are_counted_from_hop400() {
    let data = selector_only("getQueuedPegoutsCount()");
    assert!(threw(&Bridge::mainnet_up_to(Iris300).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Hop400);
    assert_eq!(int_of(&bridge.answer(&data)), 0);
    let value = one_rbtc();
    bridge.credit(crate::precompiles::BRIDGE_ADDR, value);
    bridge.value = value;
    assert!(bridge.answer(&selector_only("releaseBtc()")).is_empty());
    bridge.value = U256::ZERO;
    assert_eq!(int_of(&bridge.answer(&data)), 1);
}

/// rskj's test has a mock answer a satoshi. With no peg-out queued and
/// before RSKIP385 (fingerroot500), the real method answers 0.
#[test]
fn the_estimated_fees_for_the_next_pegout_are_answered_from_hop400() {
    let data = selector_only("getEstimatedFeesForNextPegOutEvent()");
    assert!(threw(&Bridge::mainnet_up_to(Iris300).execute(&data)));

    let mut bridge = Bridge::mainnet_up_to(Hop400);
    assert_eq!(int_of(&bridge.answer(&data)), 0);
}

/// rskj's test has a mock answer a satoshi for 1234 wei, which the real
/// method throws for: it is below the minimum peg-out. Here the amount is a
/// tenth of a bitcoin, and the federation has a bitcoin to pay it from, so
/// the simulated peg-out is built and its fee answered.
#[test]
fn the_estimated_fees_for_a_pegout_amount_are_answered_from_vetiver900() {
    let mut bridge = Bridge::mainnet_up_to(Vetiver900);
    let (_, redeem_script) = crate::bridge::peg::active_federation_keys_and_redeem(
        &mut bridge.ctx,
        &bridge.constants,
        &bridge.cfg,
        0,
    );
    let format = bridge.constants.genesis_federation_format;
    let hash160 = crate::bridge::peg::federation_output_hash160(&redeem_script, format);
    let script = [vec![0xa9, 20], hash160.to_vec(), vec![0x87]].concat();
    let utxo = crate::bridge::storage::BridgeUtxo {
        tx_hash: [0x77; 32],
        vout: 0,
        value_satoshis: 100_000_000,
        height: 1,
        script,
        coinbase: false,
    };
    let cfg = bridge.cfg.clone();
    crate::bridge::storage::store_federation_utxos(&mut bridge.ctx, &cfg, 0, &[utxo]);

    let amount = one_rbtc() / U256::from(10);
    let data = call_data("getEstimatedFeesForPegOutAmount(uint256)", &[word(amount)]);
    let fee = int_of(&bridge.answer(&data));
    assert!(fee > 0, "{fee}");
}

// ---- the union bridge (`UnionBridgeTest`) ----

/// A Bridge on `chain_id`'s network at block 0 of rskj's test configuration
/// `last`, with the network's constants.
fn union_bridge(last: RskNetworkUpgrade, chain_id: u64) -> Bridge {
    let constants = match chain_id {
        RSK_MAINNET_CHAIN_ID => BridgeConstants::mainnet(),
        RSK_TESTNET_CHAIN_ID => BridgeConstants::testnet(),
        _ => BridgeConstants::rskj_regtest(),
    };
    Bridge::new(up_to(last, chain_id), constants, 0)
}

fn union_constants(bridge: &Bridge) -> crate::bridge::union::UnionConstants {
    crate::bridge::union::union_constants(&bridge.cfg)
}

const SUCCESS: i64 = 0;
const UNAUTHORIZED_CALLER: i64 = -1;
const INVALID_VALUE: i64 = -2;
const DISABLED: i64 = -3;

/// Each of the union bridge's methods throws on lovell700, before RSKIP502
/// opens them, on testnet and mainnet alike, even from its authorizer.
#[test]
fn the_union_bridge_methods_are_closed_before_reed810() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Lovell700, chain_id);
        let constants = union_constants(&bridge);
        let new_address = Address::repeat_byte(0x5a);
        let calls = [
            (
                constants.change_address_authorizer,
                call_data(
                    "setUnionBridgeContractAddressForTestnet(address)",
                    &[address(new_address)],
                ),
            ),
            (STRANGER, selector_only("getUnionBridgeContractAddress()")),
            (STRANGER, selector_only("getUnionBridgeLockingCap()")),
            (
                constants.change_locking_cap_authorizer,
                call_data(
                    "increaseUnionBridgeLockingCap(uint256)",
                    &[word(one_rbtc() * U256::from(2))],
                ),
            ),
            (
                constants.address,
                selector_only("requestUnionBridgeRbtc(uint256)"),
            ),
            (constants.address, selector_only("releaseUnionBridgeRbtc()")),
            (
                constants.change_transfer_permissions_authorizer,
                call_data(
                    "setUnionBridgeTransferPermissions(bool,bool)",
                    &[boolean(true), boolean(true)],
                ),
            ),
        ];
        for (caller, data) in calls {
            bridge.caller = caller;
            assert!(threw(&bridge.execute(&data)), "{chain_id}: {data:?}");
        }
    }
}

/// The six event methods throw on reed800, before RSKIP529 opens them.
#[test]
fn the_union_bridge_events_are_closed_before_reed810() {
    let mut bridge = union_bridge(Reed800, RSK_MAINNET_CHAIN_ID);
    bridge.caller = union_constants(&bridge).address;
    let event = [0x56u8];
    for data in [
        selector_only("getSuperEvent()"),
        call_data("setSuperEvent(bytes)", &[Arg::Bytes(&event)]),
        selector_only("clearSuperEvent()"),
        selector_only("getBaseEvent()"),
        call_data("setBaseEvent(bytes)", &[Arg::Bytes(&event)]),
        selector_only("clearBaseEvent()"),
    ] {
        assert!(threw(&bridge.execute(&data)), "{data:?}");
    }
}

fn union_contract_address(bridge: &mut Bridge) -> Address {
    address_of(&bridge.answer(&selector_only("getUnionBridgeContractAddress()")))
}

#[test]
fn the_union_bridge_contract_address_is_set_by_its_authorizer_on_testnet() {
    let mut bridge = union_bridge(Vetiver900, RSK_TESTNET_CHAIN_ID);
    bridge.caller = union_constants(&bridge).change_address_authorizer;
    let new_address = Address::repeat_byte(0x5a);
    let data = call_data(
        "setUnionBridgeContractAddressForTestnet(address)",
        &[address(new_address)],
    );
    assert_eq!(int_of(&bridge.answer(&data)), SUCCESS);
    assert_eq!(union_contract_address(&mut bridge), new_address);
}

/// No argument reads as the zero address, which is set.
#[test]
fn no_union_bridge_contract_address_sets_the_zero_address() {
    let mut bridge = union_bridge(Vetiver900, RSK_TESTNET_CHAIN_ID);
    bridge.caller = union_constants(&bridge).change_address_authorizer;
    let set = call_data(
        "setUnionBridgeContractAddressForTestnet(address)",
        &[address(Address::repeat_byte(0x5a))],
    );
    assert_eq!(int_of(&bridge.answer(&set)), SUCCESS);
    let data = selector_only("setUnionBridgeContractAddressForTestnet(address)");
    assert_eq!(int_of(&bridge.answer(&data)), SUCCESS);
    assert_eq!(union_contract_address(&mut bridge), Address::ZERO);
}

#[test]
fn the_union_bridge_contract_address_throws_for_anyone_but_its_authorizer() {
    let mut bridge = union_bridge(Vetiver900, RSK_TESTNET_CHAIN_ID);
    bridge.caller = Address::with_last_byte(1);
    let data = call_data(
        "setUnionBridgeContractAddressForTestnet(address)",
        &[address(Address::repeat_byte(0x5a))],
    );
    assert!(threw(&bridge.execute(&data)));
    assert_eq!(
        union_contract_address(&mut bridge),
        union_constants(&bridge).address
    );
}

/// On mainnet even its authorizer, the zero address, can't set it.
#[test]
fn the_union_bridge_contract_address_is_never_set_on_mainnet() {
    let mut bridge = union_bridge(Vetiver900, RSK_MAINNET_CHAIN_ID);
    bridge.caller = union_constants(&bridge).change_address_authorizer;
    let data = call_data(
        "setUnionBridgeContractAddressForTestnet(address)",
        &[address(Address::repeat_byte(0x5a))],
    );
    assert!(threw(&bridge.execute(&data)));
}

#[test]
fn the_union_bridge_contract_address_is_its_networks() {
    for chain_id in [
        RSK_REGTEST_CHAIN_ID,
        RSK_TESTNET_CHAIN_ID,
        RSK_MAINNET_CHAIN_ID,
    ] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        assert_eq!(
            union_contract_address(&mut bridge),
            union_constants(&bridge).address
        );
    }
}

#[test]
fn the_union_bridge_locking_cap_starts_at_one_rbtc() {
    for chain_id in [
        RSK_REGTEST_CHAIN_ID,
        RSK_TESTNET_CHAIN_ID,
        RSK_MAINNET_CHAIN_ID,
    ] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        let cap = uint_of(&bridge.answer(&selector_only("getUnionBridgeLockingCap()")));
        assert_eq!(cap, one_rbtc());
    }
}

fn increase_union_cap(bridge: &mut Bridge, cap: U256) -> i64 {
    int_of(&bridge.answer(&call_data(
        "increaseUnionBridgeLockingCap(uint256)",
        &[word(cap)],
    )))
}

#[test]
fn the_union_locking_cap_may_be_doubled_by_its_authorizer() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        bridge.caller = union_constants(&bridge).change_locking_cap_authorizer;
        assert_eq!(
            increase_union_cap(&mut bridge, one_rbtc() * U256::from(2)),
            SUCCESS
        );
        let cap = uint_of(&bridge.answer(&selector_only("getUnionBridgeLockingCap()")));
        assert_eq!(cap, one_rbtc() * U256::from(2));
    }
}

#[test]
fn the_union_locking_cap_may_not_rise_past_double() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        bridge.caller = union_constants(&bridge).change_locking_cap_authorizer;
        let past_double = one_rbtc() * U256::from(2) + U256::from(1);
        assert_eq!(increase_union_cap(&mut bridge, past_double), INVALID_VALUE);
    }
}

#[test]
fn a_union_locking_cap_of_zero_is_an_invalid_value() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        bridge.caller = union_constants(&bridge).change_locking_cap_authorizer;
        assert_eq!(increase_union_cap(&mut bridge, U256::ZERO), INVALID_VALUE);
    }
}

/// No argument reads as a cap of zero.
#[test]
fn no_union_locking_cap_is_an_invalid_value() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        bridge.caller = union_constants(&bridge).change_locking_cap_authorizer;
        let data = selector_only("increaseUnionBridgeLockingCap(uint256)");
        assert_eq!(int_of(&bridge.answer(&data)), INVALID_VALUE);
    }
}

#[test]
fn the_union_locking_cap_throws_for_anyone_but_its_authorizer() {
    let mut bridge = union_bridge(Vetiver900, RSK_MAINNET_CHAIN_ID);
    bridge.caller = Address::with_last_byte(1);
    let data = call_data(
        "increaseUnionBridgeLockingCap(uint256)",
        &[word(one_rbtc() * U256::from(2))],
    );
    assert!(threw(&bridge.execute(&data)));
}

/// On mainnet the union bridge's address and its authorizers are all the
/// zero address, so one caller does what each test needs.
fn mainnet_union_bridge() -> Bridge {
    let mut bridge = union_bridge(Vetiver900, RSK_MAINNET_CHAIN_ID);
    let constants = union_constants(&bridge);
    assert_eq!(constants.address, constants.change_locking_cap_authorizer);
    assert_eq!(
        constants.address,
        constants.change_transfer_permissions_authorizer
    );
    bridge.caller = constants.address;
    bridge
}

fn request_union_rbtc(bridge: &mut Bridge, amount: U256) -> i64 {
    int_of(&bridge.answer(&call_data(
        "requestUnionBridgeRbtc(uint256)",
        &[word(amount)],
    )))
}

/// `releaseUnionBridgeRbtc` with `amount`, which the call has moved into
/// the Bridge before the Bridge runs.
fn release_union_rbtc(bridge: &mut Bridge, amount: U256) -> i64 {
    bridge.credit(crate::precompiles::BRIDGE_ADDR, amount);
    bridge.value = amount;
    let code = int_of(&bridge.answer(&selector_only("releaseUnionBridgeRbtc()")));
    bridge.value = U256::ZERO;
    code
}

fn set_union_transfer_permissions(bridge: &mut Bridge, request: bool, release: bool) {
    let data = call_data(
        "setUnionBridgeTransferPermissions(bool,bool)",
        &[boolean(request), boolean(release)],
    );
    assert_eq!(int_of(&bridge.answer(&data)), SUCCESS);
}

/// rskj's test has a mock answer success to a release of ten RBTC and checks
/// that nothing is sent back. Here the cap is raised to sixteen, ten are
/// asked for, which the union bridge's contract gets, and ten are returned:
/// the Bridge keeps them.
#[test]
fn a_release_of_what_was_requested_succeeds_and_sends_nothing_back() {
    let mut bridge = mainnet_union_bridge();
    let union = bridge.caller;
    for times in [2u64, 4, 8, 16] {
        assert_eq!(
            increase_union_cap(&mut bridge, one_rbtc() * U256::from(times)),
            SUCCESS
        );
    }
    let ten = one_rbtc() * U256::from(10);
    let held = bridge.balance(union);
    assert_eq!(request_union_rbtc(&mut bridge, ten), SUCCESS);
    assert_eq!(bridge.balance(union), held + ten);

    let bridge_held = bridge.balance(crate::precompiles::BRIDGE_ADDR);
    assert_eq!(release_union_rbtc(&mut bridge, ten), SUCCESS);
    assert_eq!(
        bridge.balance(crate::precompiles::BRIDGE_ADDR),
        bridge_held + ten
    );
}

/// A release that fails sends the value back to whoever sent it: from
/// anyone but the union bridge's contract (`UNAUTHORIZED_CALLER`), while
/// releases are off (`RELEASE_DISABLED`), and of more than was requested
/// (`INVALID_VALUE`).
#[test]
fn a_failed_release_sends_the_value_back() {
    let ten = one_rbtc() * U256::from(10);
    let bridge_address = crate::precompiles::BRIDGE_ADDR;

    let mut bridge = mainnet_union_bridge();
    bridge.caller = STRANGER;
    let held = bridge.balance(STRANGER);
    assert_eq!(release_union_rbtc(&mut bridge, ten), UNAUTHORIZED_CALLER);
    assert_eq!(bridge.balance(STRANGER), held + ten);
    assert_eq!(bridge.balance(bridge_address), U256::ZERO);

    let mut bridge = mainnet_union_bridge();
    set_union_transfer_permissions(&mut bridge, true, false);
    assert_eq!(release_union_rbtc(&mut bridge, ten), DISABLED);
    assert_eq!(bridge.balance(bridge_address), U256::ZERO);

    let mut bridge = mainnet_union_bridge();
    assert_eq!(release_union_rbtc(&mut bridge, ten), INVALID_VALUE);
    assert_eq!(bridge.balance(bridge_address), U256::ZERO);
}

/// A request that fails sends nothing: from anyone but the union bridge's
/// contract (`UNAUTHORIZED_CALLER`), while requests are off
/// (`REQUEST_DISABLED`), past the cap (`INVALID_VALUE`), and of nothing,
/// which no argument reads as (`INVALID_VALUE`).
#[test]
fn a_failed_request_sends_nothing() {
    let mut bridge = mainnet_union_bridge();
    let union = bridge.caller;
    let held = bridge.balance(union);

    bridge.caller = STRANGER;
    assert_eq!(
        request_union_rbtc(&mut bridge, one_rbtc()),
        UNAUTHORIZED_CALLER
    );
    bridge.caller = union;
    assert_eq!(
        request_union_rbtc(&mut bridge, one_rbtc() * U256::from(2)),
        INVALID_VALUE
    );
    let data = selector_only("requestUnionBridgeRbtc(uint256)");
    assert_eq!(int_of(&bridge.answer(&data)), INVALID_VALUE);
    set_union_transfer_permissions(&mut bridge, false, true);
    assert_eq!(request_union_rbtc(&mut bridge, one_rbtc()), DISABLED);

    assert_eq!(bridge.balance(union), held);
}

#[test]
fn the_union_transfer_permissions_throw_for_anyone_but_their_authorizer() {
    for chain_id in [RSK_TESTNET_CHAIN_ID, RSK_MAINNET_CHAIN_ID] {
        let mut bridge = union_bridge(Vetiver900, chain_id);
        bridge.caller = Address::with_last_byte(1);
        let data = call_data(
            "setUnionBridgeTransferPermissions(bool,bool)",
            &[boolean(true), boolean(true)],
        );
        assert!(threw(&bridge.execute(&data)));
    }
}

#[test]
fn arrowhead631_is_on_the_ladder_of_lovell700() {
    // rskj's lovell700() has arrowhead631's RSKIP434, as `up_to` schedules it.
    let cfg = mainnet_up_to(Lovell700);
    assert_eq!(cfg.activation_height(Arrowhead631), Some(0));
    assert_eq!(cfg.activation_height(RskNetworkUpgrade::Reed810), None);
}
