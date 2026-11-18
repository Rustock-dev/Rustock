//! rskj's `co.rsk.remasc.RemascFederationProviderTest`, its tests of
//! RSKIP415 (arrowhead600): REMASC pays a federator at the address of its
//! BTC public key before it, and at the address of its RSK public key from
//! it (`RemascFederationProvider.getFederatorAddress`). Here the Bridge
//! holds a federation of one member with rskj's two keys, and REMASC pays it
//! the federation's 168 at blocks 15 and 16.

use alloy_primitives::{address, hex, Address, U256};

use super::runner::{run, without_rskip85};
use crate::bridge::federation::{serialize_federation_multikey, StoredMember};
use crate::bridge::storage::{bridge_storage_key, NEW_FEDERATION_KEY};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::BRIDGE_ADDR;
use crate::rskj_tests::vm::harness::Chain;

/// rskj's federator keys, and the addresses each gives.
const BTC_PUBLIC_KEY: &str = "03a5e32151f974c35c5d08af36a8d6af0593248e8e4adca7ed0148192eb2f5d1bf";
const RSK_PUBLIC_KEY: &str = "02f3b5fb3121932db9450121b0a37f3f051dc8b3fd5f1ea5e7cf726947d8f69c28";
const FROM_BTC_KEY: Address = address!("131c7c821b0e4a1ab570feed952d9304e03fddd1");
const FROM_RSK_KEY: Address = address!("54a948b3d76ce84e5c7b4b3cd01f2af1f18d41e0");

/// The chain, its Bridge holding a federation of one member whose BTC and
/// RSK keys are rskj's.
fn paid(hardforks: RskHardforkConfig) -> Chain {
    let key = |key: &str| -> [u8; 33] {
        let bytes = hex::decode(key).expect("hex");
        bytes.try_into().expect("a compressed key")
    };
    let member = StoredMember {
        btc: key(BTC_PUBLIC_KEY),
        rsk: key(RSK_PUBLIC_KEY),
        mst: key(RSK_PUBLIC_KEY),
    };
    let federation = serialize_federation_multikey(&[member], 0, 0);
    run(hardforks, without_rskip85(), 1, &[], |chain| {
        chain.put(BRIDGE_ADDR, 0, U256::ZERO, &[]);
        let at = bridge_storage_key(NEW_FEDERATION_KEY);
        chain.put_storage_bytes(BRIDGE_ADDR, at, &federation);
    })
}

/// Before RSKIP415 the federator is paid at the address of its BTC key.
#[test]
fn before_rskip415_remasc_pays_a_federator_at_its_btc_keys_address() {
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
    let test = "getFederatorAddress_preRSKIP415_returnsRskAddressDerivedFromBtcPublicKey";
    assert_eq!(chain.balance(FROM_BTC_KEY), U256::from(2 * 168), "{test}");
    assert_eq!(chain.balance(FROM_RSK_KEY), U256::ZERO, "{test}");
}

/// From RSKIP415 the federator is paid at the address of its RSK key.
#[test]
fn from_rskip415_remasc_pays_a_federator_at_its_rsk_keys_address() {
    let chain = paid(RskHardforkConfig::rskj_regtest());
    let test = "getFederatorAddress_postRSKIP415_returnsRskAddressDerivedFromRskPublicKey";
    assert_eq!(chain.balance(FROM_RSK_KEY), U256::from(2 * 168), "{test}");
    assert_eq!(chain.balance(FROM_BTC_KEY), U256::ZERO, "{test}");
}
