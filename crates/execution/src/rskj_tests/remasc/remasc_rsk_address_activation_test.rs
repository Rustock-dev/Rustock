//! rskj's `co.rsk.remasc.RemascRskAddressActivationTest`: RSKIP218 (iris300)
//! moves RSK Labs' cut of each reward to a new address
//! (`Remasc.getRskLabsAddress`).

use alloy_primitives::{address, Address, U256};

use super::runner::{run, without_rskip85, LAST};
use crate::hardfork::RskHardforkConfig;

/// rskj's two addresses for RSK Labs, before RSKIP218 and from it.
const BEFORE: Address = address!("14d3065c8eb89895f4df12450ec6b130049f8034");
const FROM: Address = address!("dcb12179ba4697350f66224c959bdd9c282818df");

/// With RSKIP218 at block 16, block 15 pays RSK Labs' 4,200 at the first
/// address and block 16 at the second.
#[test]
fn rsk_labs_cut_goes_to_its_rskip218_address_from_rskip218() {
    let mut remasc = without_rskip85();
    remasc.rsk_labs_address = BEFORE;
    remasc.rsk_labs_address_rskip218 = FROM;
    remasc.rskip218_activation_height = Some(LAST);
    let chain = run(RskHardforkConfig::rskj_regtest(), remasc, 1, &[], |_| {});
    assert_eq!(chain.balance(BEFORE), U256::from(4_200), "testActivation");
    assert_eq!(chain.balance(FROM), U256::from(4_200), "testActivation");
}
