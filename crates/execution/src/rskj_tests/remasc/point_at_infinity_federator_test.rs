//! A federator whose key is the point at infinity, the one byte `00`.
//!
//! Not a port: no test of rskj's has one. rskj's
//! `RemascFederationProvider.getFederatorAddress` reads the federator's key
//! with `ECKey.fromPublicOnly`, for which Bouncy Castle decodes `00` as that
//! point, and pays `getAddress()` of it: the last 20 bytes of the Keccak-256
//! hash of nothing, since the point's uncompressed encoding is the one byte
//! the address leaves out. The key is the BTC key before RSKIP415, the RSK
//! key from it. Here the Bridge holds a federation of two members, one with
//! that BTC key and one with that RSK key, and REMASC pays each half of the
//! federation's 168 at blocks 15 and 16.

use alloy_primitives::{address, hex, Address, U256};

use super::runner::{run, without_rskip85};
use crate::bridge::serialization::{rlp_encode_element, rlp_encode_list, rlp_encode_u64};
use crate::bridge::storage::{bridge_storage_key, NEW_FEDERATION_KEY};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::BRIDGE_ADDR;
use crate::rskj_tests::vm::harness::Chain;

/// rskj's federator keys of `RemascFederationProviderTest`, and the
/// addresses each gives.
const BTC_PUBLIC_KEY: &str = "03a5e32151f974c35c5d08af36a8d6af0593248e8e4adca7ed0148192eb2f5d1bf";
const RSK_PUBLIC_KEY: &str = "02f3b5fb3121932db9450121b0a37f3f051dc8b3fd5f1ea5e7cf726947d8f69c28";
const FROM_BTC_KEY: Address = address!("131c7c821b0e4a1ab570feed952d9304e03fddd1");
const FROM_RSK_KEY: Address = address!("54a948b3d76ce84e5c7b4b3cd01f2af1f18d41e0");

/// The point at infinity's address.
const INFINITY: Address = address!("dcc703c0e500b653ca82273b7bfad8045d85a470");

/// The chain, its Bridge holding a federation of two members, as rskj
/// serializes it: the first of BTC key `00` and RSK and MST key rskj's RSK
/// key, the second of BTC and MST key rskj's BTC key and RSK key `00`.
fn paid(hardforks: RskHardforkConfig) -> Chain {
    let btc = hex::decode(BTC_PUBLIC_KEY).expect("hex");
    let rsk = hex::decode(RSK_PUBLIC_KEY).expect("hex");
    let member = |btc: &[u8], rsk: &[u8], mst: &[u8]| {
        rlp_encode_element(&rlp_encode_list(&[
            rlp_encode_element(btc),
            rlp_encode_element(rsk),
            rlp_encode_element(mst),
        ]))
    };
    let federation = rlp_encode_list(&[
        rlp_encode_u64(0),
        rlp_encode_u64(0),
        rlp_encode_list(&[member(&[0x00], &rsk, &rsk), member(&btc, &[0x00], &btc)]),
    ]);
    run(hardforks, without_rskip85(), 1, &[], |chain| {
        chain.put(BRIDGE_ADDR, 0, U256::ZERO, &[]);
        let at = bridge_storage_key(NEW_FEDERATION_KEY);
        chain.put_storage_bytes(BRIDGE_ADDR, at, &federation);
    })
}

/// Before RSKIP415 each federator is paid at its BTC key's address: the
/// first at the point at infinity's.
#[test]
fn before_rskip415_a_federator_of_btc_key_00_is_paid_at_its_address() {
    let mut hardforks = RskHardforkConfig::rskj_regtest();
    for upgrade in [
        RskNetworkUpgrade::Arrowhead600,
        RskNetworkUpgrade::Arrowhead631,
        RskNetworkUpgrade::Lovell700,
        RskNetworkUpgrade::Reed800,
        RskNetworkUpgrade::Reed810,
        RskNetworkUpgrade::Vetiver900,
    ] {
        hardforks = hardforks.with_hardfork(upgrade, None);
    }
    let chain = paid(hardforks);
    assert_eq!(chain.balance(INFINITY), U256::from(168));
    assert_eq!(chain.balance(FROM_BTC_KEY), U256::from(168));
    assert_eq!(chain.balance(FROM_RSK_KEY), U256::ZERO);
}

/// From RSKIP415 each federator is paid at its RSK key's address: the
/// second at the point at infinity's.
#[test]
fn from_rskip415_a_federator_of_rsk_key_00_is_paid_at_its_address() {
    let chain = paid(RskHardforkConfig::rskj_regtest());
    assert_eq!(chain.balance(FROM_RSK_KEY), U256::from(168));
    assert_eq!(chain.balance(INFINITY), U256::from(168));
    assert_eq!(chain.balance(FROM_BTC_KEY), U256::ZERO);
}
