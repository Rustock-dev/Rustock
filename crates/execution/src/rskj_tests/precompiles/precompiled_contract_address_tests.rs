//! rskj's `co.rsk.vm.precompiles.PrecompiledContractAddressTests`: the
//! precompiled contract each address holds once RSKIP106, RSKIP119 and
//! RSKIP516 are on, as they are from reed800 on mainnet.

use alloy_primitives::{hex, Address};

use super::harness::call_precompile;
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::{
    rsk_precompiles, BLOCK_HEADER_ADDR, BRIDGE_ADDR, HD_WALLET_UTILS_ADDR, REMASC_ADDR,
    SECP256K1_ADD_ADDR, SECP256K1_MUL_ADDR,
};

/// reed800 on mainnet: RSKIP516.
const REED: u64 = 8_052_200;

/// Each of the eleven addresses rskj names holds a precompiled contract, and
/// those of sha256, ripemd160 and identity answer rskj's own vectors as
/// those contracts do.
#[test]
fn each_address_holds_its_precompiled_contract() {
    let set = rsk_precompiles(&RskHardforkConfig::mainnet(), REED);
    let ethereums = [1, 2, 3, 4, 5].map(Address::with_last_byte);
    let rsks = [
        BRIDGE_ADDR,
        REMASC_ADDR,
        BLOCK_HEADER_ADDR,
        HD_WALLET_UTILS_ADDR,
        SECP256K1_ADD_ADDR,
        SECP256K1_MUL_ADDR,
    ];
    for address in ethereums.into_iter().chain(rsks) {
        assert!(set.contains(&address), "testGetPrecompile: {address}");
    }
    let sha256 = call_precompile(REED, Address::with_last_byte(2), 1_000, &[]);
    let hash = hex!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256, (true, hash.to_vec()), "Sha256");
    let mut one = [0u8; 32];
    one[31] = 1;
    let ripempd160 = call_precompile(REED, Address::with_last_byte(3), 1_000, &one);
    let hash = hex!("000000000000000000000000ae387fcfeb723c3f5964509af111cf5a67f30661");
    assert_eq!(ripempd160, (true, hash.to_vec()), "Ripempd160");
    let input = hex!("112233445566");
    let identity = call_precompile(REED, Address::with_last_byte(4), 1_000, &input);
    assert_eq!(identity, (true, input.to_vec()), "Identity");
}
