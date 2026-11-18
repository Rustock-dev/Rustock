//! rskj `co.rsk.peg.BridgeUtilsLegacyTest`: the utilities rskj ran before
//! RSKIP284, RSKIP293 and RSKIP271, through the Bridge methods that reach
//! them.

use alloy_primitives::Address as RskAddress;

use super::support::{self, flyover_code, FlyoverCall};
use crate::bridge::constants::BridgeConstants;
use crate::hardfork::RskHardforkConfig;

/// Mainnet's first block with RSKIP271 (hop400).
const MAINNET_RSKIP271: u64 = 4_598_500;

/// rskj `BridgeUtilsLegacy.calculatePegoutTxSize`, the size of the regular
/// peg-out (two inputs, two outputs) from RSKIP219 until RSKIP271
/// (BridgeUtilsLegacy.java:42-55): for each input its signatures at 72 bytes
/// each, its redeem script and 41 bytes; 35 bytes for each output, and 4. For
/// 13 members it is 2,054 bytes, where from RSKIP271 bitcoinj's serialization
/// gives 2,058. `releaseBtc` asks a peg-out to be worth the fee per KB times
/// that size, with the network's percentage on top (BridgeSupport.java:
/// 958-983): what the legacy size asks is queued at the last block before
/// RSKIP271, and is short at its first.
#[test]
fn before_rskip271_the_legacy_pegout_size_sets_releasebtcs_minimum() {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let keys = support::keys(1, 13);
    let redeem = support::standard_redeem(&keys);
    let legacy_size = 4 + 2 * (7 * 72 + redeem.len() + 1 + 40) + 2 * (25 + 1 + 9);
    assert_eq!(legacy_size, 2054);
    let fee_per_kb = 1_000_000u64;
    let minimum = |size: u64| {
        let fee = fee_per_kb * size / 1000;
        fee + fee * config.minimum_pegout_value_percentage_to_receive_after_fee / 100
    };
    let queued = |block, satoshis| {
        support::release_queued((&config, &cfg), block, &keys, fee_per_kb, satoshis)
    };
    assert_eq!(queued(MAINNET_RSKIP271 - 1, minimum(2054)), 1);
    assert_eq!(queued(MAINNET_RSKIP271 - 1, minimum(2054) - 1), 0);
    assert_eq!(queued(MAINNET_RSKIP271, minimum(2054)), 0);
    assert_eq!(queued(MAINNET_RSKIP271, minimum(2058)), 1);
}

/// What `registerFastBridgeBtcTransaction` answers for the two Bitcoin
/// addresses `user` and `lp`, with a transaction it never gets to.
fn answer(net: (BridgeConstants, RskHardforkConfig), block: u64, user: &[u8], lp: &[u8]) -> i128 {
    let call = FlyoverCall {
        tx: &[1, 0, 0, 0, 1],
        height: i64::from(i32::MAX),
        derivation_arguments: [0x11; 32],
        user,
        lbc: RskAddress::repeat_byte(0x42),
        lp,
    };
    call.answer(&mut support::ctx_at(block), &net.0, &net.1)
}

/// rskj `BridgeUtilsLegacy.deserializeBtcAddressWithVersionLegacy`, which
/// `Bridge.registerFlyoverBtcTransaction` reads both its Bitcoin addresses
/// with before RSKIP284 (BridgeUtilsLegacy.java:59-79): no bytes throw, and
/// the hash is the 20 bytes after the version, which throws for fewer
/// (`System.arraycopy`). bitcoinj then refuses a version its network doesn't
/// take, and the method answers GENERIC_ERROR for each. The blocks are past
/// iris300 and before RSKIP284: testnet #2,122,593 and mainnet #4,000,000.
#[test]
fn before_rskip284_a_flyover_address_is_refused_short_or_of_another_network() {
    let testnet = || (BridgeConstants::testnet(), RskHardforkConfig::testnet());
    let mainnet = || (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let with = |version: u8| [&[version][..], &[0x41; 20][..]].concat();
    for (case, net, block, user, lp) in [
        ("no bytes", testnet(), 2_122_593, vec![], with(0x6f)),
        (
            "a hash of one byte",
            testnet(),
            2_122_593,
            vec![0x6f, 0x41],
            with(0x6f),
        ),
        (
            "a testnet p2sh version on mainnet",
            mainnet(),
            4_000_000,
            with(0xc4),
            with(0x00),
        ),
        (
            "a mainnet p2sh version on testnet",
            testnet(),
            2_122_593,
            with(0x05),
            with(0x6f),
        ),
    ] {
        assert_eq!(
            answer(net, block, &user, &lp),
            flyover_code::GENERIC_ERROR,
            "{case}"
        );
    }
    // The same addresses on their own networks get past them.
    assert_ne!(
        answer(mainnet(), 4_000_000, &with(0x05), &with(0x00)),
        flyover_code::GENERIC_ERROR
    );
    assert_ne!(
        answer(testnet(), 2_122_593, &with(0x6f), &with(0x6f)),
        flyover_code::GENERIC_ERROR
    );
}
