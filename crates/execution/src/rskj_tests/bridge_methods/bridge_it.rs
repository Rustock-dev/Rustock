//! rskj's `BridgeIT`: the Bridge on rskj's regtest, from a genesis that has
//! nothing active, with an upgrade's rules switched on where a test switches
//! them on.
//!
//! rskj's tests switch single RSKIPs on (`doReturn(true).when(activationConfig)
//! .isActive(eq(RSKIPxx), anyLong())`). A network switches them on with their
//! upgrade, and so does this port: orchid where a test turns on RSKIP87, 88
//! or 89, wasabi100 where it turns on RSKIP122, 123 or 124. Where a test turns
//! a later rule on and leaves RSKIP88 off, a combination no network's upgrades
//! make, the port takes the upgrade's, with RSKIP88 on, and the method then
//! throws where rskj's test has it answer null.

use super::support::*;
use crate::bridge::federation::{
    rsk_address_from_public_key, serialize_federation_multikey, serialize_federation_only_btc_keys,
    StoredMember,
};
use crate::bridge::serialization::{rlp_encode_element, rlp_encode_list, rlp_encode_u64};
use crate::bridge::storage::{
    bridge_load_bytes_named, bridge_store_bytes_named, NEW_FEDERATION_FORMAT_VERSION_KEY,
    NEW_FEDERATION_KEY, OLD_FEDERATION_FORMAT_VERSION_KEY, OLD_FEDERATION_KEY,
    PEGOUTS_WAITING_FOR_SIGNATURES_KEY, PENDING_FEDERATION_KEY,
    PROPOSED_FEDERATION_FORMAT_VERSION_KEY, PROPOSED_FEDERATION_KEY,
};
use alloy_primitives::{Address, Bytes, U256};
use revm::precompile::{PrecompileError, PrecompileOutput};
use RskNetworkUpgrade::{Genesis, Iris300, Orchid, Vetiver900, Wasabi100};

/// At block `number` of rskj's regtest, every upgrade to `last` from block 0.
fn regtest_at(last: RskNetworkUpgrade, number: u64) -> Bridge {
    Bridge::new(
        up_to(last, RSK_REGTEST_CHAIN_ID),
        BridgeConstants::rskj_regtest(),
        number,
    )
}

fn regtest(last: RskNetworkUpgrade) -> Bridge {
    regtest_at(last, 0)
}

/// What rskj's `bridge.execute` answers null with: no output.
fn null(
    result: Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError>,
) -> bool {
    matches!(result, Ok(output) if output.bytes.is_empty())
}

/// Keys that are points on the curve: mainnet's genesis federation's.
fn keys() -> Vec<[u8; 33]> {
    BridgeConstants::mainnet()
        .genesis_federation_public_keys
        .iter()
        .map(|key| {
            alloy_primitives::hex::decode(key)
                .unwrap()
                .try_into()
                .unwrap()
        })
        .collect()
}

/// Three members whose BTC, RSK and MST keys differ, from `first` on.
fn members(first: usize) -> Vec<StoredMember> {
    let keys = keys();
    let mut members: Vec<StoredMember> = (first..first + 3)
        .map(|i| StoredMember {
            btc: keys[i],
            rsk: keys[i + 6],
            mst: keys[i + 9],
        })
        .collect();
    members.sort();
    members
}

/// A retiring federation of `retiring`, with an active one created after
/// it that the Bridge's block has let take over. Before RSKIP123 rskj keeps
/// a federation's BTC keys only; from it, each member's three keys.
fn store_retiring_federation(bridge: &mut Bridge, retiring: &[StoredMember], multikey: bool) {
    let active = members(3);
    let serialize = |members: &[StoredMember], time: u64, block: u64| {
        if multikey {
            serialize_federation_multikey(members, time, block)
        } else {
            let btc: Vec<[u8; 33]> = members.iter().map(|m| m.btc).collect();
            serialize_federation_only_btc_keys(&btc, time, block)
        }
    };
    bridge_store_bytes_named(
        &mut bridge.ctx,
        OLD_FEDERATION_KEY,
        &serialize(retiring, 1_000, 1),
    );
    bridge_store_bytes_named(
        &mut bridge.ctx,
        NEW_FEDERATION_KEY,
        &serialize(&active, 2_000, 2),
    );
}

/// A pending federation of `pending`: its BTC keys before RSKIP123, each
/// member's three keys from it.
fn store_pending_federation(bridge: &mut Bridge, pending: &[StoredMember], multikey: bool) {
    let items: Vec<Vec<u8>> = if multikey {
        pending.iter().map(StoredMember::to_rlp).collect()
    } else {
        pending.iter().map(|m| rlp_encode_element(&m.btc)).collect()
    };
    bridge_store_bytes_named(
        &mut bridge.ctx,
        PENDING_FEDERATION_KEY,
        &rlp_encode_list(&items),
    );
}

fn key_of_type(member: &StoredMember, key_type: &str) -> Vec<u8> {
    match key_type {
        "btc" => member.btc.to_vec(),
        "rsk" => member.rsk.to_vec(),
        _ => member.mst.to_vec(),
    }
}

fn index(signature: &str, index: usize) -> Vec<u8> {
    call_data(signature, &[int(index as i64)])
}

fn index_and_type(signature: &str, index: usize, key_type: &str) -> Vec<u8> {
    call_data(
        signature,
        &[int(index as i64), Arg::Bytes(key_type.as_bytes())],
    )
}

const KEY_TYPES: [&str; 3] = ["btc", "rsk", "mst"];

#[test]
fn a_call_too_short_for_a_selector_is_no_call_before_orchid_and_throws_from_it() {
    assert!(null(regtest(Genesis).execute(&[0; 3])));
    assert!(threw(&regtest(Orchid).execute(&[0; 3])));
}

#[test]
fn a_selector_of_no_method_is_no_call_before_orchid_and_throws_from_it() {
    assert!(null(regtest(Genesis).execute(&[0; 4])));
    assert!(threw(&regtest(Orchid).execute(&[0; 4])));
}

/// rskj's test has a mock answer one hash. The chain here is the one the
/// Bridge starts from on regtest, Bitcoin's regtest genesis block alone, so
/// the locator is its hash.
#[test]
fn the_block_locator_is_the_chains_before_orchid() {
    let mut bridge = regtest(Genesis);
    let locator = strings_of(&bridge.answer(&selector_only("getBtcBlockchainBlockLocator()")));
    let genesis = bitcoin::constants::genesis_block(bitcoin::Network::Regtest);
    assert_eq!(locator, vec![genesis.header.block_hash().to_string()]);
}

#[test]
fn the_block_locator_is_closed_from_orchid() {
    let mut bridge = regtest(Orchid);
    bridge.local_call = true;
    assert!(threw(
        &bridge.execute(&selector_only("getBtcBlockchainBlockLocator()"))
    ));
}

/// rskj's test has a mock answer each index's bytes. Here each index of the
/// genesis federation answers that member's key, and one past them throws.
#[test]
fn each_federators_key_is_answered_before_wasabi100() {
    let mut bridge = regtest(Genesis);
    let keys = crate::bridge::peg::genesis_federation_keys(&bridge.constants);
    let signature = "getFederatorPublicKey(int256)";
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(bytes_of(&bridge.answer(&index(signature, i))), key.to_vec());
    }
    assert!(threw(&bridge.execute(&index(signature, keys.len()))));
}

#[test]
fn the_federators_key_is_closed_from_wasabi100() {
    let mut bridge = regtest(Wasabi100);
    bridge.local_call = true;
    assert!(threw(
        &bridge.execute(&index("getFederatorPublicKey(int256)", 0))
    ));
}

#[test]
fn the_federators_key_of_a_type_is_no_call_before_wasabi100() {
    let data = index_and_type("getFederatorPublicKeyOfType(int256,string)", 0, "btc");
    assert!(null(regtest(Genesis).execute(&data)));
}

/// A genesis member has one key for all three types.
#[test]
fn the_federators_key_of_each_type_is_answered_from_wasabi100() {
    let mut bridge = regtest(Wasabi100);
    bridge.local_call = true;
    let first = crate::bridge::peg::genesis_federation_keys(&bridge.constants)[0];
    let signature = "getFederatorPublicKeyOfType(int256,string)";
    for key_type in KEY_TYPES {
        let key = bytes_of(&bridge.answer(&index_and_type(signature, 0, key_type)));
        assert_eq!(key, first.to_vec());
    }
    assert!(threw(&bridge.execute(&index_and_type(signature, 3, "btc"))));
}

#[test]
fn a_retiring_federators_key_is_answered_before_wasabi100() {
    let mut bridge = regtest_at(Genesis, 50);
    let retiring = members(0);
    store_retiring_federation(&mut bridge, &retiring, false);
    let key = bytes_of(&bridge.answer(&index("getRetiringFederatorPublicKey(int256)", 0)));
    assert_eq!(key, retiring[0].btc.to_vec());
}

#[test]
fn no_retiring_federation_has_an_empty_key_before_wasabi100() {
    let mut bridge = regtest_at(Genesis, 50);
    let key = bytes_of(&bridge.answer(&index("getRetiringFederatorPublicKey(int256)", 0)));
    assert!(key.is_empty());
}

#[test]
fn the_retiring_federators_key_is_closed_from_wasabi100() {
    let mut bridge = regtest_at(Wasabi100, 50);
    bridge.local_call = true;
    let data = index("getRetiringFederatorPublicKey(int256)", 10);
    assert!(threw(&bridge.execute(&data)));
}

#[test]
fn the_retiring_federators_key_of_a_type_is_no_call_before_wasabi100() {
    let mut bridge = regtest_at(Genesis, 50);
    for key_type in KEY_TYPES {
        let data = index_and_type(
            "getRetiringFederatorPublicKeyOfType(int256,string)",
            10,
            key_type,
        );
        assert!(null(bridge.execute(&data)));
    }
}

#[test]
fn a_retiring_federators_key_of_each_type_is_answered_from_wasabi100() {
    let mut bridge = regtest_at(Wasabi100, 50);
    bridge.local_call = true;
    let retiring = members(0);
    store_retiring_federation(&mut bridge, &retiring, true);
    let signature = "getRetiringFederatorPublicKeyOfType(int256,string)";
    for key_type in KEY_TYPES {
        let key = bytes_of(&bridge.answer(&index_and_type(signature, 0, key_type)));
        assert_eq!(key, key_of_type(&retiring[0], key_type));
    }
}

#[test]
fn no_retiring_federation_has_an_empty_key_of_each_type_from_wasabi100() {
    let mut bridge = regtest_at(Wasabi100, 50);
    bridge.local_call = true;
    let signature = "getRetiringFederatorPublicKeyOfType(int256,string)";
    for key_type in KEY_TYPES {
        assert!(bytes_of(&bridge.answer(&index_and_type(signature, 0, key_type))).is_empty());
    }
}

/// rskj's test has a mock answer each index's bytes. Here a pending
/// federation is stored, each of its indexes answers its member's key, and
/// one past them throws.
#[test]
fn each_pending_federators_key_is_answered_before_wasabi100() {
    let mut bridge = regtest(Genesis);
    let pending = members(0);
    store_pending_federation(&mut bridge, &pending, false);
    let signature = "getPendingFederatorPublicKey(int256)";
    for (i, member) in pending.iter().enumerate() {
        assert_eq!(
            bytes_of(&bridge.answer(&index(signature, i))),
            member.btc.to_vec()
        );
    }
    assert!(threw(&bridge.execute(&index(signature, pending.len()))));
}

#[test]
fn the_pending_federators_key_is_closed_from_wasabi100() {
    let mut bridge = regtest(Wasabi100);
    bridge.local_call = true;
    assert!(threw(
        &bridge.execute(&index("getPendingFederatorPublicKey(int256)", 10))
    ));
}

#[test]
fn the_pending_federators_key_of_a_type_is_no_call_before_wasabi100() {
    let data = index_and_type(
        "getPendingFederatorPublicKeyOfType(int256,string)",
        10,
        "btc",
    );
    assert!(null(regtest(Genesis).execute(&data)));
}

#[test]
fn a_pending_federators_key_of_each_type_is_answered_from_wasabi100() {
    let mut bridge = regtest(Wasabi100);
    bridge.local_call = true;
    let pending = members(0);
    store_pending_federation(&mut bridge, &pending, true);
    let signature = "getPendingFederatorPublicKeyOfType(int256,string)";
    for (i, member) in pending.iter().enumerate() {
        for key_type in KEY_TYPES {
            let key = bytes_of(&bridge.answer(&index_and_type(signature, i, key_type)));
            assert_eq!(key, key_of_type(member, key_type));
        }
    }
}

/// rskj's test has a mock count the vote. The real vote, from no
/// authorizer, is turned down: -10.
#[test]
fn a_federator_public_key_is_voted_before_wasabi100() {
    let data = call_data(
        "addFederatorPublicKey(bytes)",
        &[Arg::Bytes(&[0xaa, 0xbb, 0xcc, 0xdd])],
    );
    assert_eq!(int_of(&regtest(Genesis).answer(&data)), -10);
}

#[test]
fn a_federator_public_key_is_voted_no_more_from_wasabi100() {
    let data = call_data(
        "addFederatorPublicKey(bytes)",
        &[Arg::Bytes(&[0xaa, 0xbb, 0xcc, 0xdd])],
    );
    assert!(threw(&regtest(Wasabi100).execute(&data)));
}

fn add_multikey_data() -> Vec<u8> {
    call_data(
        "addFederatorPublicKeyMultikey(bytes,bytes,bytes)",
        &[
            Arg::Bytes(&[0xaa, 0xbb]),
            Arg::Bytes(&[0xcc, 0xdd]),
            Arg::Bytes(&[0xee, 0xff]),
        ],
    )
}

#[test]
fn a_federators_three_keys_are_no_call_before_wasabi100() {
    assert!(null(regtest(Genesis).execute(&add_multikey_data())));
}

/// rskj's test has a mock count the vote. The real vote, from no
/// authorizer, is turned down: -10.
#[test]
fn a_federators_three_keys_are_voted_from_wasabi100() {
    assert_eq!(
        int_of(&regtest(Wasabi100).answer(&add_multikey_data())),
        -10
    );
}

/// A regtest P2PKH address.
fn regtest_address(tag: u8) -> String {
    bitcoin::base58::encode_check(&[vec![111], vec![tag; 20]].concat())
}

fn entry_by_address(address: &str) -> Vec<u8> {
    call_data(
        "getLockWhitelistEntryByAddress(string)",
        &[Arg::Bytes(address.as_bytes())],
    )
}

#[test]
fn a_whitelist_entry_is_no_call_before_orchid() {
    assert!(null(
        regtest(Genesis).execute(&entry_by_address(&regtest_address(1)))
    ));
}

/// rskj's test has a mock hold an unlimited and a one-off entry. Here the
/// whitelist's authorizer adds them, and they are read back: 0 for the
/// unlimited one, the one-off's value, and -1 for an address not listed.
#[test]
fn whitelist_entries_are_answered_from_orchid() {
    let mut bridge = regtest(Orchid);
    bridge.caller = address_of_key(bridge.constants.whitelist_authorizer_keys[0]);
    let (unlimited, one_off) = (regtest_address(1), regtest_address(2));
    let add_unlimited = call_data(
        "addUnlimitedLockWhitelistAddress(string)",
        &[Arg::Bytes(unlimited.as_bytes())],
    );
    assert_eq!(int_of(&bridge.answer(&add_unlimited)), 1);
    let add_one_off = call_data(
        "addOneOffLockWhitelistAddress(string,int256)",
        &[Arg::Bytes(one_off.as_bytes()), int(100_000_000)],
    );
    assert_eq!(int_of(&bridge.answer(&add_one_off)), 1);

    bridge.local_call = true;
    assert_eq!(int_of(&bridge.answer(&entry_by_address(&unlimited))), 0);
    assert_eq!(
        int_of(&bridge.answer(&entry_by_address(&one_off))),
        100_000_000
    );
    assert_eq!(
        int_of(&bridge.answer(&entry_by_address(&regtest_address(3)))),
        -1
    );
}

fn add_lock_whitelist_data(address: &str) -> Vec<u8> {
    call_data(
        "addLockWhitelistAddress(string,int256)",
        &[Arg::Bytes(address.as_bytes()), int(100_000_000)],
    )
}

/// From a federator, who is no authorizer of the whitelist's: -10.
#[test]
fn a_whitelist_address_from_no_authorizer_is_turned_down_before_orchid() {
    let mut bridge = regtest(Genesis);
    bridge.caller = genesis_federator(&bridge.constants);
    assert_eq!(
        int_of(&bridge.answer(&add_lock_whitelist_data(&regtest_address(1)))),
        -10
    );
}

#[test]
fn add_lock_whitelist_address_is_closed_from_orchid() {
    assert!(threw(
        &regtest(Orchid).execute(&add_lock_whitelist_data("i-am-an-address"))
    ));
}

fn add_one_off_data(address: &str) -> Vec<u8> {
    call_data(
        "addOneOffLockWhitelistAddress(string,int256)",
        &[Arg::Bytes(address.as_bytes()), int(100_000_000)],
    )
}

#[test]
fn a_one_off_whitelist_address_is_no_call_before_orchid() {
    assert!(null(
        regtest(Genesis).execute(&add_one_off_data("i-am-an-address"))
    ));
}

/// From a federator, who is no authorizer of the whitelist's: -10.
#[test]
fn a_one_off_whitelist_address_from_no_authorizer_is_turned_down_from_orchid() {
    let mut bridge = regtest(Orchid);
    bridge.caller = genesis_federator(&bridge.constants);
    assert_eq!(
        int_of(&bridge.answer(&add_one_off_data(&regtest_address(1)))),
        -10
    );
}

fn add_unlimited_data(address: &str) -> Vec<u8> {
    call_data(
        "addUnlimitedLockWhitelistAddress(string)",
        &[Arg::Bytes(address.as_bytes())],
    )
}

#[test]
fn an_unlimited_whitelist_address_is_no_call_before_orchid() {
    assert!(null(
        regtest(Genesis).execute(&add_unlimited_data("i-am-an-address"))
    ));
}

/// From a federator, who is no authorizer of the whitelist's: -10.
#[test]
fn an_unlimited_whitelist_address_from_no_authorizer_is_turned_down_from_orchid() {
    let mut bridge = regtest(Orchid);
    bridge.caller = genesis_federator(&bridge.constants);
    assert_eq!(
        int_of(&bridge.answer(&add_unlimited_data(&regtest_address(1)))),
        -10
    );
}

/// rskj's test replays the data of mainnet block 457's first federator
/// call, kept in a file of rskj's: `receiveHeaders` with five hundred
/// elements of 81 bytes, each a header and a count of no transactions,
/// 80,068 bytes in all. It checks that the call decodes and runs before
/// orchid without asking whether it is a local call. The call here has that
/// shape, from a federator, not a local call.
#[test]
fn a_call_of_five_hundred_headers_decodes_and_runs_before_orchid() {
    let mut bridge = regtest(Genesis);
    bridge.caller = genesis_federator(&bridge.constants);
    let headers: Vec<Vec<u8>> = (0..500u32)
        .map(|i| [i.to_le_bytes().to_vec(), vec![0; 77]].concat())
        .collect();
    let headers: Vec<&[u8]> = headers.iter().map(Vec::as_slice).collect();
    let data = call_data("receiveHeaders(bytes[])", &[Arg::BytesArray(&headers)]);
    assert_eq!(data.len(), 80_068);
    assert!(null(bridge.execute(&data)));
}

#[test]
fn a_local_only_method_takes_a_transaction_before_orchid() {
    let mut bridge = regtest(Genesis);
    let address = string_of(&bridge.answer(&selector_only("getFederationAddress()")));
    assert!(
        address.starts_with('2'),
        "a regtest P2SH address: {address}"
    );
}

#[test]
fn a_local_only_method_throws_for_a_transaction_from_orchid() {
    let mut bridge = regtest(Orchid);
    assert!(threw(
        &bridge.execute(&selector_only("getFederationAddress()"))
    ));
}

#[test]
fn btc_transaction_confirmations_are_no_call_before_wasabi100() {
    let data = call_data(
        "getBtcTransactionConfirmations(bytes32,bytes32,uint256,bytes32[])",
        &[
            Arg::Word([0; 32]),
            Arg::Word([0; 32]),
            int(1),
            Arg::Words(&[[0; 32]]),
        ],
    );
    assert!(null(regtest(Genesis).execute(&data)));
}

/// What `getGasForData` charges for a call: its method's cost and two gas a
/// byte (`Bridge.getGasForData`).
fn gas_for(bridge: &mut Bridge, data: &[u8]) -> u64 {
    crate::bridge::bridge_call_gas_cost(&mut bridge.ctx, data, &bridge.constants, &bridge.cfg)
        .unwrap()
}

/// rskj's test has a mock cost the lookup 1,234. The real one costs 27,000
/// for a block the chain doesn't have, and for one it has, 315 more for
/// each block above it and 144 for each hash of the branch.
#[test]
fn the_gas_for_btc_transaction_confirmations_is_two_a_byte_and_the_lookups() {
    use sha2::Digest;
    let hash = |data: &[u8]| -> [u8; 32] { sha2::Sha256::digest(data).into() };
    let branch = [hash(&[0x11]), hash(&[0x22]), hash(&[0x33])];
    let signature = "getBtcTransactionConfirmations(bytes32,bytes32,uint256,bytes32[])";
    let mut bridge = regtest(Wasabi100);

    let data = call_data(
        signature,
        &[
            Arg::Word(hash(&[0xaa, 0xbb, 0xcc])),
            Arg::Word(hash(&[0xdd, 0xee, 0xff])),
            int(123),
            Arg::Words(&branch),
        ],
    );
    assert_eq!(gas_for(&mut bridge, &data), 2 * data.len() as u64 + 27_000);

    let genesis = bitcoin::constants::genesis_block(bitcoin::Network::Regtest);
    let genesis_hash = crate::bridge::btc_chain::bitcoin_hash_to_b256(&genesis.header.block_hash());
    let data = call_data(
        signature,
        &[
            Arg::Word(hash(&[0xaa, 0xbb, 0xcc])),
            Arg::Word(genesis_hash.0),
            int(123),
            Arg::Words(&branch),
        ],
    );
    assert_eq!(
        gas_for(&mut bridge, &data),
        2 * data.len() as u64 + 27_000 + 3 * 144
    );
}

/// A transaction to a contract that DELEGATECALLs the Bridge's
/// `getMinimumLockTxValue`, as rskj's test assembles it, forwarding 0x6000
/// gas, on rskj's regtest at block 1 of `last`. Whether it ended well and
/// the gas it used.
fn a_contract_calls_get_minimum_lock_tx_value(last: RskNetworkUpgrade) -> (bool, u64) {
    use alloy_primitives::{Bloom, B256};
    use rustock_trie::{
        account_key, code_key, AccountState, MemoryTrieStore, TrieKeySlice, TrieNode, TrieStore,
    };
    let selector = selector_only("getMinimumLockTxValue()");
    let mut code = vec![0x7f];
    code.extend_from_slice(&[selector, vec![0; 28]].concat());
    code.extend_from_slice(&[0x60, 0x00, 0x52]); // MSTORE at 0
    code.extend_from_slice(&[0x60, 0x20, 0x60, 0x30, 0x60, 0x20, 0x60, 0x00]); // out, in
    code.push(0x73);
    code.extend_from_slice(crate::precompiles::BRIDGE_ADDR.as_slice());
    code.extend_from_slice(&[0x61, 0x60, 0x00, 0xf4, 0x00]); // gas, DELEGATECALL, STOP

    let (sender, contract) = (Address::repeat_byte(0xaa), Address::repeat_byte(0xbb));
    let store = std::sync::Arc::new(MemoryTrieStore::new());
    let put = |root: &TrieNode, address: &Address, nonce: u64, balance: U256| {
        let key = account_key(address);
        let account = AccountState::new(U256::from(nonce), balance);
        root.put(
            &TrieKeySlice::from_key(&key),
            &account.encode(),
            store.as_ref() as &dyn TrieStore,
        )
    };
    let root = put(
        &TrieNode::empty(),
        &sender,
        0,
        U256::from(10u64).pow(U256::from(18)),
    );
    let root = put(&root, &contract, 1, U256::ZERO);
    let root = root.put(
        &TrieKeySlice::from_key(&code_key(&contract)),
        &code,
        store.as_ref(),
    );

    let number = 1;
    let header = rustock_core::Header {
        parent_hash: B256::ZERO,
        ommers_hash: B256::ZERO,
        beneficiary: Address::repeat_byte(0x01),
        state_root: B256::ZERO,
        transactions_root: B256::ZERO,
        receipts_root: B256::ZERO,
        logs_bloom: Bloom::ZERO,
        extension_data: None,
        difficulty: U256::from(1_000_000),
        number,
        gas_limit: U256::from(6_800_000),
        gas_used: 0,
        timestamp: 1_700_000_000,
        extra_data: Bytes::new(),
        paid_fees: U256::ZERO,
        minimum_gas_price: U256::ZERO,
        uncle_count: 0,
        umm_root: rustock_core::config::ActivationHeights::current()
            .umm_active(number)
            .then(Bytes::new),
        version_fields: None,
        bitcoin_merged_mining_header: None,
        bitcoin_merged_mining_merkle_proof: None,
        bitcoin_merged_mining_coinbase_transaction: None,
        cached_hash: None,
        cached_hash_for_merged_mining: None,
        kept: None,
    };
    let tx = rustock_core::Transaction {
        nonce: 0,
        gas_price: U256::ZERO,
        gas_limit: U256::from(1_000_000),
        to: Bytes::copy_from_slice(contract.as_slice()),
        value: U256::ZERO,
        input: Bytes::new(),
        v: 0,
        r: U256::ZERO,
        s: U256::ZERO,
        cached_rlp: None,
    };
    let blocks = rustock_storage::BlockStore::open(tempfile::tempdir().unwrap().path()).unwrap();
    let executor = crate::executor::RskExecutor::new(
        up_to(last, RSK_REGTEST_CHAIN_ID),
        std::sync::Arc::new(blocks),
    );
    let result = executor
        .execute_block(&header, &[(tx, sender)], &root, store)
        .unwrap();
    (result.tx_results[0].success, result.tx_results[0].gas_used)
}

/// Before orchid the Bridge can't price a contract's call
/// (`getGasForData` reads a transaction it hasn't got), and from orchid
/// it throws for a contract's call of a local-only method. Before RSKIP197
/// (iris300) either leaves the VM and fails the contract with all its gas;
/// from it the call fails and the contract goes on.
#[test]
fn a_contracts_call_of_a_local_only_method_fails_the_contract_before_iris300() {
    assert_eq!(
        a_contract_calls_get_minimum_lock_tx_value(Genesis),
        (false, 1_000_000)
    );
    assert_eq!(
        a_contract_calls_get_minimum_lock_tx_value(Orchid),
        (false, 1_000_000)
    );
    let (ended_well, gas) = a_contract_calls_get_minimum_lock_tx_value(Iris300);
    assert!(ended_well && gas < 1_000_000, "{gas}");
}

fn headers_data(count: usize) -> Vec<u8> {
    let header = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
    let headers: Vec<&[u8]> = vec![&header[..]; count];
    call_data("receiveHeaders(bytes[])", &[Arg::BytesArray(&headers)])
}

#[test]
fn receive_headers_costs_22000_and_two_a_byte_before_wasabi100() {
    let mut bridge = regtest(Genesis);
    for count in 0..10 {
        let data = headers_data(count);
        assert_eq!(gas_for(&mut bridge, &data), 22_000 + 2 * data.len() as u64);
    }
}

/// RSKIP124 without RSKIP132 is testnet's first blocks: wasabi100 from block
/// 0, RSKIP132 from 43,550.
#[test]
fn receive_headers_costs_66000_and_1650_a_header_past_the_first_with_rskip124() {
    let mut bridge = Bridge::new(RskHardforkConfig::testnet(), BridgeConstants::testnet(), 0);
    assert!(bridge.cfg.has_rskip124(0) && !bridge.cfg.has_rskip132(0));
    for count in 0..10u64 {
        let data = headers_data(count as usize);
        let per_header = 1_650 * count.saturating_sub(1);
        assert_eq!(
            gas_for(&mut bridge, &data),
            66_000 + 2 * data.len() as u64 + per_header
        );
    }
}

fn access_headers_data() -> Vec<u8> {
    let header = alloy_primitives::hex::decode(
        "0000002006226e46111a0b59caaf126043eb5bbf28c34f3a5e332a1fc7b2b73cf188910ff698ee112158f5573a90b7403cfba074addd61c547b3639c6afdcf52588eb8e2a1ef825cffff7f2000000000",
    )
    .unwrap();
    call_data("receiveHeaders(bytes[])", &[Arg::BytesArray(&[&header])])
}

/// The header as rskj's test sends it, and nothing but the selector, as
/// `receiveHeadersNotFromTheFederation` sends it.
#[test]
fn receive_headers_turns_a_stranger_down_before_wasabi100() {
    let mut bridge = regtest(Genesis);
    bridge.caller = Address::from_slice(
        &alloy_primitives::hex::decode("2acc95758f8b5f583470ba265eb685a8f45fc9d5").unwrap(),
    );
    assert!(threw(&bridge.execute(&access_headers_data())));
    assert!(threw(
        &bridge.execute(&selector_only("receiveHeaders(bytes[])"))
    ));
}

#[test]
fn receive_headers_takes_a_federator_before_wasabi100() {
    let mut bridge = regtest(Genesis);
    bridge.caller = genesis_federator(&bridge.constants);
    assert!(null(bridge.execute(&access_headers_data())));
}

#[test]
fn receive_headers_takes_anyone_from_wasabi100() {
    assert!(null(regtest(Wasabi100).execute(&access_headers_data())));
}

/// rskj's `getGasForData` tests, from a genesis with nothing active: a parse
/// failure costs releaseBtc's 23,000 and nothing a byte; a method, its cost
/// and two gas a byte.
#[test]
fn the_gas_for_a_call_is_its_methods_cost_and_two_a_byte() {
    let mut bridge = regtest(Genesis);
    assert_eq!(gas_for(&mut bridge, &[1, 2, 3]), 23_000);
    for (signature, cost) in [
        ("updateCollections()", 48_000),
        ("receiveHeaders(bytes[])", 22_000),
        ("releaseBtc()", 23_000),
        ("getStateForBtcReleaseClient()", 4_000),
        ("getStateForDebugging()", 3_000_000),
        ("getBtcBlockchainBestChainHeight()", 19_000),
        ("getBtcBlockchainBlockLocator()", 76_000),
        ("getFederationAddress()", 11_000),
        ("getMinimumLockTxValue()", 2_000),
    ] {
        assert_eq!(
            gas_for(&mut bridge, &selector_only(signature)),
            cost + 8,
            "{signature}"
        );
    }
    let register = call_data(
        "registerBtcTransaction(bytes,int256,bytes)",
        &[Arg::Bytes(&[0; 3]), int(1), Arg::Bytes(&[0; 3])],
    );
    assert_eq!(register.len(), 228);
    assert_eq!(gas_for(&mut bridge, &register), 22_000 + 228 * 2);
    let add_signature = call_data(
        "addSignature(bytes,bytes[],bytes)",
        &[
            Arg::Bytes(&[0; 3]),
            Arg::BytesArray(&[&[0; 2], &[0; 2], &[0; 2]]),
            Arg::Bytes(&[0; 3]),
        ],
    );
    assert_eq!(add_signature.len(), 548);
    assert_eq!(gas_for(&mut bridge, &add_signature), 70_000 + 548 * 2);
}

/// rskj's `localCallOnlyMethodsDefinition` and `mineableMethodsDefinition`.
#[test]
fn the_local_only_methods_and_the_mineable_ones_are_rskjs() {
    use crate::bridge::BridgePermission::LocalOnly;
    let permission = |signature: &str| {
        let selector = crate::abi::selector(signature);
        crate::bridge::find_bridge_method(&selector)
            .unwrap()
            .permission
    };
    for signature in [
        "getBtcBlockchainInitialBlockHeight()",
        "getBtcBlockchainBlockLocator()",
        "getBtcBlockchainBlockHashAtDepth(int256)",
        "getBtcTxHashProcessedHeight(string)",
        "getFederationAddress()",
        "getFederationCreationBlockNumber()",
        "getFederationCreationTime()",
        "getFederationSize()",
        "getFederationThreshold()",
        "getFederatorPublicKey(int256)",
        "getFeePerKb()",
        "getLockWhitelistAddress(int256)",
        "getLockWhitelistEntryByAddress(string)",
        "getLockWhitelistSize()",
        "getMinimumLockTxValue()",
        "getPendingFederationHash()",
        "getPendingFederationSize()",
        "getPendingFederatorPublicKey(int256)",
        "getRetiringFederationAddress()",
        "getRetiringFederationCreationBlockNumber()",
        "getRetiringFederationCreationTime()",
        "getRetiringFederationSize()",
        "getRetiringFederationThreshold()",
        "getRetiringFederatorPublicKey(int256)",
        "getStateForBtcReleaseClient()",
        "getStateForDebugging()",
        "isBtcTxHashAlreadyProcessed(string)",
    ] {
        assert_eq!(permission(signature), LocalOnly, "{signature}");
    }
    for signature in [
        "addFederatorPublicKey(bytes)",
        "addLockWhitelistAddress(string,int256)",
        "addOneOffLockWhitelistAddress(string,int256)",
        "addUnlimitedLockWhitelistAddress(string)",
        "addSignature(bytes,bytes[],bytes)",
        "commitFederation(bytes)",
        "createFederation()",
        "receiveHeaders(bytes[])",
        "registerBtcTransaction(bytes,int256,bytes)",
        "releaseBtc()",
        "removeLockWhitelistAddress(string)",
        "rollbackFederation()",
        "setLockWhitelistDisableBlockDelay(int256)",
        "updateCollections()",
        "voteFeePerKbChange(int256)",
        "getActiveFederationCreationBlockHeight()",
    ] {
        assert_ne!(permission(signature), LocalOnly, "{signature}");
    }
}

/// rskj's `callUpdateCollectionsWithSignatureNotFromFederation`: before
/// orchid, `updateCollections` from anyone outside the federations throws,
/// and a federator's runs. rskj also reads the exception's message; a throw
/// here carries none.
#[test]
fn update_collections_throws_for_a_caller_outside_the_federations() {
    let mut bridge = regtest(Genesis);
    assert!(threw(
        &bridge.execute(&selector_only("updateCollections()"))
    ));
    bridge.caller = genesis_federator(&bridge.constants);
    assert!(null(bridge.execute(&selector_only("updateCollections()"))));
}

/// A Bitcoin transaction of one input and one output of `value` satoshis.
fn btc_tx(tag: u8, value: u64) -> Vec<u8> {
    use bitcoin::hashes::Hash;
    bitcoin::consensus::serialize(&bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([tag; 32]),
                vout: 0,
            },
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(value),
            script_pubkey: bitcoin::ScriptBuf::new_p2pkh(&bitcoin::PubkeyHash::from_byte_array(
                [tag; 20],
            )),
        }],
    })
}

/// rskj's `callUpdateCollectionsWithTransactionsWaitingForConfirmation` and
/// `…WithEnoughConfirmations`: peg-outs made at blocks 1, 2 and 3 wait for
/// regtest's three confirmations. At block 0 none has them; at block 10 all
/// have, and a federator's call moves one on to be signed. The limit
/// between: the one made at block 1 has its three at block 4.
#[test]
fn update_collections_moves_one_confirmed_pegout_on_to_be_signed() {
    use crate::bridge::peg::{
        deserialize_rsk_txs_waiting_for_signatures, load_pegout_confirmation_set,
        store_pegout_confirmation_set, PegoutWaitingForConfirmations,
    };
    let minimum = BridgeConstants::rskj_regtest().minimum_pegout_tx_value;
    let after_a_call_at = |number: u64| {
        let mut bridge = regtest_at(Genesis, number);
        let pegouts: Vec<PegoutWaitingForConfirmations> = (1..=3u8)
            .map(|made_at| PegoutWaitingForConfirmations {
                btc_tx_raw: btc_tx(made_at + 1, minimum + 100_000 * u64::from(made_at - 1)),
                rsk_block_height: u64::from(made_at),
                rsk_tx_hash: None,
            })
            .collect();
        store_pegout_confirmation_set(&mut bridge.ctx, &pegouts, false);
        bridge.caller = genesis_federator(&bridge.constants);
        assert!(null(bridge.execute(&selector_only("updateCollections()"))));
        let waiting = load_pegout_confirmation_set(&mut bridge.ctx, false).len();
        let signing = bridge_load_bytes_named(&mut bridge.ctx, PEGOUTS_WAITING_FOR_SIGNATURES_KEY);
        (
            waiting,
            deserialize_rsk_txs_waiting_for_signatures(&signing).len(),
        )
    };
    assert_eq!(after_a_call_at(0), (3, 0));
    assert_eq!(after_a_call_at(10), (2, 1));
    assert_eq!(after_a_call_at(3), (3, 0));
    assert_eq!(after_a_call_at(4), (2, 1));
}

/// rskj's `getPegoutWaitingForSignatureKey`: a confirmed peg-out waits to be
/// signed under the transaction that made it from RSKIP146, under the call
/// that moves it on from RSKIP176 ("moving back to using the
/// updateCollections related txHash"), and under the one that made it again
/// from RSKIP375.
#[test]
fn from_rskip176_a_confirmed_pegout_waits_to_be_signed_under_the_call_that_moves_it() {
    use crate::bridge::peg::{
        deserialize_rsk_txs_waiting_for_signatures, store_pegout_confirmation_set,
        PegoutWaitingForConfirmations,
    };
    let made_by = [0x55u8; 32];
    let key_after_a_call_under = |last: RskNetworkUpgrade| {
        let mut bridge = regtest_at(last, 10);
        let minimum = bridge.constants.minimum_pegout_tx_value;
        let pegout = PegoutWaitingForConfirmations {
            btc_tx_raw: btc_tx(2, minimum),
            rsk_block_height: 1,
            rsk_tx_hash: Some(made_by),
        };
        store_pegout_confirmation_set(&mut bridge.ctx, &[pegout], true);
        bridge.caller = genesis_federator(&bridge.constants);
        assert!(null(bridge.execute(&selector_only("updateCollections()"))));
        let signing = bridge_load_bytes_named(&mut bridge.ctx, PEGOUTS_WAITING_FOR_SIGNATURES_KEY);
        let keys: Vec<[u8; 32]> = deserialize_rsk_txs_waiting_for_signatures(&signing)
            .into_keys()
            .collect();
        keys
    };
    // The call's own hash is the default context's, all zeros.
    assert_eq!(
        key_after_a_call_under(RskNetworkUpgrade::Papyrus200),
        vec![made_by]
    );
    assert_eq!(key_after_a_call_under(Iris300), vec![[0u8; 32]]);
    assert_eq!(
        key_after_a_call_under(RskNetworkUpgrade::Hop400),
        vec![[0u8; 32]]
    );
    assert_eq!(key_after_a_call_under(Vetiver900), vec![made_by]);
}

/// A header of Bitcoin's regtest on its genesis block, mined to regtest's
/// easiest target.
fn header_on_genesis() -> Vec<u8> {
    use bitcoin::hashes::Hash;
    let genesis = bitcoin::constants::genesis_block(bitcoin::Network::Regtest);
    let header = (0..)
        .map(|nonce| bitcoin::block::Header {
            version: bitcoin::block::Version::from_consensus(1),
            prev_blockhash: genesis.block_hash(),
            merkle_root: bitcoin::TxMerkleNode::from_byte_array([0x11; 32]),
            time: 1_700_000_000,
            bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
            nonce,
        })
        .find(|header| header.validate_pow(header.target()).is_ok())
        .unwrap();
    bitcoin::consensus::serialize(&header)
}

fn one_header(header: &[u8]) -> Vec<u8> {
    call_data("receiveHeaders(bytes[])", &[Arg::BytesArray(&[header])])
}

/// What a federator's call answers before orchid, and the height of the
/// Bridge's chain after it.
fn receive_before_orchid(data: &[u8]) -> (bool, i64) {
    let mut bridge = regtest(Genesis);
    bridge.caller = genesis_federator(&bridge.constants);
    let answered_null = null(bridge.execute(data));
    let height = selector_only("getBtcBlockchainBestChainHeight()");
    (answered_null, int_of(&bridge.answer(&height)))
}

/// rskj's `receiveHeadersWithCorrectSizeHeaders` and the three tests that
/// hand it what isn't a header: before RSKIP124 a header is its 80 bytes,
/// and may be followed by a count of no transactions up to 85 bytes in all.
/// Anything else is no call before orchid, and the chain takes nothing from
/// it: rskj's 60 bytes, its count of `Integer.MAX_VALUE` transactions, its
/// whole block of two transactions, and one byte short of or past the
/// limits.
#[test]
fn before_wasabi100_a_header_is_80_to_85_bytes_with_no_transactions() {
    let header = header_on_genesis();
    let with = |tail: &[u8]| [header.as_slice(), tail].concat();
    for taken in [header.clone(), with(&[0]), with(&[0xfe, 0, 0, 0, 0])] {
        let size = taken.len();
        assert_eq!(
            receive_before_orchid(&one_header(&taken)),
            (true, 1),
            "{size} bytes"
        );
    }
    let block = [
        header.as_slice(),
        &[2],
        &btc_tx(0x01, 5_000_000_000),
        &btc_tx(0x02, 100_000_000),
    ]
    .concat();
    for refused in [
        header[..60].to_vec(),
        header[..79].to_vec(),
        with(&[0xfe, 0xff, 0xff, 0xff, 0x7f]),
        with(&[0xfe, 0, 0, 0, 0, 0]),
        block,
    ] {
        let size = refused.len();
        assert_eq!(
            receive_before_orchid(&one_header(&refused)),
            (true, 0),
            "{size} bytes"
        );
    }
}

/// rskj's `sendNoBlockHeader`: `receiveHeaders` with nothing but its
/// selector reads as no headers (rskj decodes an empty payload's array as
/// empty), runs, and receives none, before orchid and from it.
#[test]
fn receive_headers_with_nothing_but_its_selector_receives_no_header() {
    let data = selector_only("receiveHeaders(bytes[])");
    assert_eq!(receive_before_orchid(&data), (true, 0));
    let mut bridge = regtest(Orchid);
    bridge.caller = genesis_federator(&bridge.constants);
    assert!(null(bridge.execute(&data)));
}

/// rskj's `sendOrphanBlockHeader`: a header whose parent the chain hasn't
/// got leaves the chain's height as it was.
#[test]
fn an_orphan_header_leaves_the_chain_as_it_was() {
    let orphan = [
        1u32.to_le_bytes().to_vec(),
        vec![0x01; 32],
        vec![0x02; 32],
        1u32.to_le_bytes().to_vec(),
        0x207f_ffffu32.to_le_bytes().to_vec(),
        1u32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(receive_before_orchid(&one_header(&orphan)), (true, 0));
}

/// rskj's `registerBtcTransactionNotFromFederation`: before iris300 a caller
/// outside the federations throws.
#[test]
fn register_btc_transaction_throws_for_a_caller_outside_the_federations() {
    let data = call_data(
        "registerBtcTransaction(bytes,int256,bytes)",
        &[Arg::Bytes(&[0; 3]), int(1), Arg::Bytes(&[0; 30])],
    );
    assert!(threw(&regtest(Genesis).execute(&data)));
}

/// rskj's `HugeDeclaredSizeBtcTransaction`: a transaction of one input, one
/// output and a one-byte witness push, written with one of its four counts,
/// `huge`, as `Integer.MAX_VALUE`.
fn huge_declared_size(huge: usize) -> Vec<u8> {
    let count = |which: usize| {
        if which == huge {
            vec![0xfe, 0xff, 0xff, 0xff, 0x7f]
        } else {
            vec![1]
        }
    };
    [
        vec![1, 0, 0, 0, 0, 1],
        count(0),
        vec![0x11; 32],
        vec![0; 5],
        vec![0xff; 4],
        count(1),
        100_000_000u64.to_le_bytes().to_vec(),
        vec![0x19, 0x76, 0xa9, 0x14],
        vec![0x22; 20],
        vec![0x88, 0xac],
        count(2),
        count(3),
        vec![0],
        vec![0; 4],
    ]
    .concat()
}

/// rskj's partial merkle tree of three transactions that declares
/// `Integer.MAX_VALUE` hashes and holds three.
fn huge_declared_size_pmt() -> Vec<u8> {
    let hash = |n: u8| [vec![n], vec![0; 31]].concat();
    [
        3u32.to_le_bytes().to_vec(),
        vec![0xfe, 0xff, 0xff, 0xff, 0x7f],
        hash(1),
        hash(2),
        hash(3),
        vec![1, 0x3f],
    ]
    .concat()
}

/// rskj's `registerBtcTransactionWithNonParseableTx`, its four
/// `…WithHugeDeclared…Size` tests, its two `…WithNonParseableMerkleeProof`
/// tests and `…WithHugeDeclaredSizeMerkleeProof`: before orchid, a
/// federator's call with a transaction or a proof the Bridge can't read
/// answers nothing. rskj reads neither: on a chain at regtest's genesis,
/// height 1 has no confirmations, which it asks first.
#[test]
fn register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid() {
    let register = |tx: &[u8], pmt: &[u8]| {
        let mut bridge = regtest(Genesis);
        bridge.caller = genesis_federator(&bridge.constants);
        let data = call_data(
            "registerBtcTransaction(bytes,int256,bytes)",
            &[Arg::Bytes(tx), int(1), Arg::Bytes(pmt)],
        );
        null(bridge.execute(&data))
    };
    assert!(register(&[0; 3], &[0; 30]));
    for huge in 0..4 {
        assert!(
            register(&huge_declared_size(huge), &[0; 30]),
            "count {huge}"
        );
    }
    let tx = btc_tx(0x33, 100_000_000);
    for pmt in [vec![0; 3], vec![0; 30], huge_declared_size_pmt()] {
        assert!(register(&tx, &pmt));
    }
}

/// A DER signature: r and s of 1.
const DER: &[u8] = &[0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01];

fn add_signature_data(key: &[u8], signatures: &[&[u8]], hash: &[u8]) -> Vec<u8> {
    call_data(
        "addSignature(bytes,bytes[],bytes)",
        &[
            Arg::Bytes(key),
            Arg::BytesArray(signatures),
            Arg::Bytes(hash),
        ],
    )
}

/// rskj's `addSignatureWithNonParseablePublicKey`, `…WithEmptySignatureArray`,
/// `…WithNonParseableSignature` and `…WithNonParseableRskTx`: from a
/// federator before orchid, a key bitcoinj can't read, no signatures, a
/// signature that isn't DER and a hash that isn't 32 bytes are no call.
/// From orchid each throws, while the call with all four sound runs.
#[test]
fn add_signature_is_no_call_for_what_it_cannot_read_before_orchid() {
    let key = crate::bridge::peg::genesis_federation_keys(&BridgeConstants::rskj_regtest())[0];
    let from_a_federator = |upgrade: RskNetworkUpgrade, data: &[u8]| {
        let mut bridge = regtest(upgrade);
        bridge.caller = genesis_federator(&bridge.constants);
        bridge.execute(data)
    };
    let unread: [(&[u8], &[&[u8]], &[u8]); 4] = [
        (&[0; 3], &[], &[0; 32]),
        (&key, &[], &[0; 32]),
        (&key, &[&[0; 3]], &[0; 32]),
        (&key, &[DER], &[0; 3]),
    ];
    for (key, signatures, hash) in unread {
        let data = add_signature_data(key, signatures, hash);
        assert!(null(from_a_federator(Genesis, &data)));
        assert!(threw(&from_a_federator(Orchid, &data)));
    }
    let sound = add_signature_data(&key, &[DER], &[0; 32]);
    assert!(null(from_a_federator(Orchid, &sound)));
}

/// rskj's `isBtcTxHashAlreadyProcessed_exception` and
/// `getBtcTxHashProcessedHeight_exception`: a hash that isn't 64 hex digits
/// throws, before orchid and from it. One that is gets its answer.
#[test]
fn a_btc_tx_hash_that_is_not_one_throws() {
    let hash = "0000000000000000000000000000000000000000000000000000000000000001";
    for signature in [
        "isBtcTxHashAlreadyProcessed(string)",
        "getBtcTxHashProcessedHeight(string)",
    ] {
        let data = |text: &str| call_data(signature, &[Arg::Bytes(text.as_bytes())]);
        for upgrade in [Genesis, Orchid] {
            let mut bridge = regtest(upgrade);
            bridge.local_call = true;
            assert!(
                threw(&bridge.execute(&data("notahash"))),
                "{signature} at {upgrade:?}"
            );
            assert_eq!(
                bridge.answer(&data(hash)).len(),
                32,
                "{signature} at {upgrade:?}"
            );
        }
    }
}

/// rskj's `getBtcTransactionConfirmationsAfterWasabi_merkleBranchConstructionError`,
/// without its mock: a path with more significant bits than the branch has
/// hashes can't make rskj's `MerkleBranch`, and the call throws. With as
/// many bits as hashes it answers, here that the block is unknown: -1.
#[test]
fn confirmations_throw_for_a_path_longer_than_its_branch() {
    let data = |path: i64| {
        call_data(
            "getBtcTransactionConfirmations(bytes32,bytes32,uint256,bytes32[])",
            &[
                Arg::Word([0xaa; 32]),
                Arg::Word([0xdd; 32]),
                int(path),
                Arg::Words(&[[0x11; 32], [0x22; 32], [0x33; 32]]),
            ],
        )
    };
    let mut bridge = regtest(Wasabi100);
    assert!(threw(&bridge.execute(&data(123))));
    assert!(threw(&bridge.execute(&data(0b1000))));
    assert_eq!(int_of(&bridge.answer(&data(0b111))), -1);
}

/// The nine keys rskj's `addSignature` tests make from `<seed>1` to
/// `<seed>9`, sorted as it sorts them.
fn seeded_keys(seed: &str) -> Vec<[u8; 33]> {
    let mut keys: Vec<[u8; 33]> = (1..=9)
        .map(|i| key_from_seed(&format!("{seed}{i}")))
        .collect();
    keys.sort();
    keys
}

/// Where a federation is stored, where its format is, and the seed of its
/// members' keys.
type Stored = (&'static str, &'static str, &'static str);
const NEW: Stored = (
    NEW_FEDERATION_KEY,
    NEW_FEDERATION_FORMAT_VERSION_KEY,
    "activeFedMember",
);
const OLD: Stored = (
    OLD_FEDERATION_KEY,
    OLD_FEDERATION_FORMAT_VERSION_KEY,
    "retiringFedMember",
);
const PROPOSED: Stored = (
    PROPOSED_FEDERATION_KEY,
    PROPOSED_FEDERATION_FORMAT_VERSION_KEY,
    "proposedFedMember",
);

/// Mainnet at `number` with every upgrade, holding `federations` as rskj's
/// `P2shErpFederationBuilder` builds them: created at block 1, in the P2SH
/// ERP format.
fn mainnet_with(number: u64, federations: &[Stored]) -> Bridge {
    let cfg = up_to(Vetiver900, RSK_MAINNET_CHAIN_ID);
    let mut bridge = Bridge::new(cfg, BridgeConstants::mainnet(), number);
    for (key, format_key, seed) in federations {
        let members: Vec<StoredMember> = seeded_keys(seed)
            .into_iter()
            .map(StoredMember::from_btc)
            .collect();
        let stored = serialize_federation_multikey(&members, 100_000_000_000, 1);
        bridge_store_bytes_named(&mut bridge.ctx, format_key, &rlp_encode_u64(3000));
        bridge_store_bytes_named(&mut bridge.ctx, key, &stored);
    }
    bridge
}

/// `addSignature` from the holder of `key`, signing with it.
fn add_signature_from(
    bridge: &mut Bridge,
    key: [u8; 33],
) -> Result<PrecompileOutput, PrecompileError> {
    bridge.caller = rsk_address_from_public_key(&key).unwrap();
    bridge.execute(&add_signature_data(&key, &[DER], &[0; 32]))
}

/// Whether the first member of the federation from `seed` may sign: the
/// method runs, or it throws.
fn signs(bridge: &mut Bridge, seed: &str) -> bool {
    let result = add_signature_from(bridge, seeded_keys(seed)[0]);
    assert!(result.is_ok() || threw(&result), "{result:?}");
    result.is_ok()
}

/// rskj's four `addSignature_…whenNewAndOldFederations…` tests: the new
/// federation, made at block 1, is active from mainnet's activation age
/// later, 40,321. From there its members and the old one's may sign; a block
/// before, only the old one's.
#[test]
fn add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side() {
    for (number, new_signs) in [(40_321, true), (40_320, false)] {
        let mut bridge = mainnet_with(number, &[NEW, OLD]);
        assert_eq!(signs(&mut bridge, NEW.2), new_signs, "at {number}");
        assert!(signs(&mut bridge, OLD.2), "at {number}");
    }
}

/// rskj's `addSignature_fromNewFederator_whenNoOldFederation_shouldCall`.
#[test]
fn add_signature_takes_a_new_federation_with_no_old_one() {
    assert!(signs(&mut mainnet_with(0, &[NEW]), NEW.2));
}

/// rskj's `addSignature_fromProposedFederator_whenProposedFederation_shouldCall`
/// and `…whenNoProposedFederation_shouldThrow`.
#[test]
fn add_signature_takes_the_proposed_federation_while_there_is_one() {
    assert!(signs(&mut mainnet_with(0, &[PROPOSED]), PROPOSED.2));
    assert!(!signs(&mut mainnet_with(0, &[]), PROPOSED.2));
}

/// rskj's `addSignature_fromNotFederatorSigner_shouldThrow`.
#[test]
fn add_signature_throws_for_a_signer_in_no_federation() {
    let mut bridge = mainnet_with(0, &[NEW, OLD, PROPOSED]);
    let signer = key_from_seed("notFederator");
    assert!(threw(&add_signature_from(&mut bridge, signer)));
}
