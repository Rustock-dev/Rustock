//! rskj `co.rsk.peg.federation.PendingFederationTest`: the federation a
//! commit builds from the pending one, in the format of the commit's era,
//! and the commits that throw because Bitcoin could not spend from what
//! they would build.
//!
//! rskj builds the federation as the commit's deciding vote goes through
//! (`PendingFederation.buildFederation`), so these tests vote as the
//! network's federation-change authorizers do: create, add each member,
//! commit. What the commit built is read from the address its
//! `commit_federation` event logs.

use alloy_primitives::{B256, U256};
use revm::context_interface::{ContextTr, JournalTr};
use sha3::{Digest as _, Keccak256};

use super::support;
use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::governance;
use crate::hardfork::RskHardforkConfig;

type Network = (BridgeConstants, RskHardforkConfig, bitcoin::Network);

fn mainnet() -> Network {
    (
        BridgeConstants::mainnet(),
        RskHardforkConfig::mainnet(),
        bitcoin::Network::Bitcoin,
    )
}

fn testnet() -> Network {
    (
        BridgeConstants::testnet(),
        RskHardforkConfig::testnet(),
        bitcoin::Network::Testnet,
    )
}

/// A block of each era a commit builds in. Mainnet: before RSKIP201
/// (iris300); from it, before RSKIP293 (hop400); from that, before RSKIP353;
/// from RSKIP353, before RSKIP305 (reed800); from RSKIP305. Testnet: before
/// RSKIP201; from RSKIP293, before RSKIP353; from RSKIP353; from RSKIP305.
const MAINNET_STANDARD: u64 = 3_000_000;
const MAINNET_NON_STANDARD_BEFORE_RSKIP293: u64 = 4_000_000;
const MAINNET_NON_STANDARD: u64 = 4_700_000;
const MAINNET_P2SH_ERP: u64 = 5_000_000;
const MAINNET_P2SH_P2WSH_ERP: u64 = 8_100_000;
const TESTNET_STANDARD: u64 = 2_000_000;
const TESTNET_NON_STANDARD: u64 = 3_200_000;
const TESTNET_P2SH_ERP: u64 = 4_000_000;
const TESTNET_P2SH_P2WSH_ERP: u64 = 6_900_000;

/// Mainnet's and testnet's delay, 52,560, pushed before RSKIP293 as two
/// unsigned big-endian bytes, and as a Java number from it and in every P2SH
/// ERP script.
const DELAY_BEFORE_RSKIP293: &[u8] = &[0xcd, 0x50];
const DELAY: &[u8] = &[0x50, 0xcd, 0x00];

/// `count` members, with the keys of the private keys 100, 200, 300, ..., as
/// rskj's tests make them.
fn members(count: u32) -> Vec<[u8; 33]> {
    (1..=count)
        .map(|i| support::public_key(&(i * 100).to_be_bytes()))
        .collect()
}

/// The string at head word `index` of an ABI-encoded event.
fn abi_string(data: &[u8], index: usize) -> String {
    let word = |at: usize| U256::from_be_slice(&data[at..at + 32]).to::<usize>();
    let at = word(32 * index);
    let length = word(at);
    String::from_utf8(data[at + 32..at + 32 + length].to_vec()).expect("an address")
}

/// What committing a pending federation of `members` at `block` answers
/// (`None` where the deciding vote throws), and the address its
/// `commit_federation` event gives the federation built.
fn commit(network: &Network, block: u64, members: &[[u8; 33]]) -> (Option<i128>, Option<String>) {
    let mut ctx = support::ctx_at(block);
    let answer = support::elect_federation(&mut ctx, (&network.0, &network.1), members);
    let topic = B256::from_slice(&Keccak256::digest(
        b"commit_federation(bytes,string,bytes,string,int256)",
    ));
    let address = ctx
        .journal_mut()
        .take_logs()
        .iter()
        .find(|log| log.data.topics().first() == Some(&topic))
        .map(|log| abi_string(&log.data.data, 3));
    (answer, address)
}

/// A federation's address, the P2SH of its redeem script.
fn p2sh(redeem: &[u8], network: bitcoin::Network) -> String {
    bitcoin::Address::p2sh(bitcoin::Script::from_bytes(redeem), network)
        .expect("a redeem script within 520 bytes")
        .to_string()
}

/// A P2SH-P2WSH federation's address, the P2SH of the witness program of its
/// redeem script.
fn p2sh_p2wsh(redeem: &[u8], network: bitcoin::Network) -> String {
    bitcoin::Address::p2shwsh(bitcoin::Script::from_bytes(redeem), network).to_string()
}

/// The federations a commit builds, by era.
#[derive(Clone, Copy, Debug)]
enum Built {
    Standard,
    /// With the delay pushed as these bytes.
    NonStandardErp(&'static [u8]),
    P2shErp,
    P2shP2wshErp,
}

/// The address rskj gives the federation of `members` built as `built`.
fn address_of(
    built: Built,
    members: &[[u8; 33]],
    emergency: &[[u8; 33]],
    net: bitcoin::Network,
) -> String {
    match built {
        Built::Standard => p2sh(&support::standard_redeem(members), net),
        Built::NonStandardErp(delay) => p2sh(
            &support::non_standard_erp_redeem(members, emergency, delay),
            net,
        ),
        Built::P2shErp => p2sh(&support::p2sh_erp_redeem(members, emergency, DELAY), net),
        Built::P2shP2wshErp => {
            p2sh_p2wsh(&support::p2sh_erp_redeem(members, emergency, DELAY), net)
        }
    }
}

/// rskj's `buildFederation_with9Members_*`, `with15Members_*` and
/// `with20Members_*`: a standard multisig before RSKIP201; a non-standard
/// ERP federation from it, before RSKIP353; a P2SH ERP federation from
/// RSKIP353, before RSKIP305; a P2SH-P2WSH ERP federation from RSKIP305. The
/// ERP federations have the network's emergency keys and delay.
#[test]
fn a_commit_builds_the_federation_of_its_era() {
    let mainnet_eras = [
        (Built::Standard, MAINNET_STANDARD, 9),
        (Built::Standard, MAINNET_STANDARD, 15),
        (
            Built::NonStandardErp(DELAY_BEFORE_RSKIP293),
            MAINNET_NON_STANDARD_BEFORE_RSKIP293,
            9,
        ),
        (Built::NonStandardErp(DELAY), MAINNET_NON_STANDARD, 9),
        (Built::P2shErp, MAINNET_P2SH_ERP, 9),
        (Built::P2shP2wshErp, MAINNET_P2SH_P2WSH_ERP, 9),
        (Built::P2shP2wshErp, MAINNET_P2SH_P2WSH_ERP, 20),
    ];
    let testnet_eras = [
        (Built::Standard, TESTNET_STANDARD, 9),
        (Built::Standard, TESTNET_STANDARD, 15),
        (Built::NonStandardErp(DELAY), TESTNET_NON_STANDARD, 9),
        (Built::P2shErp, TESTNET_P2SH_ERP, 9),
        (Built::P2shP2wshErp, TESTNET_P2SH_P2WSH_ERP, 9),
        (Built::P2shP2wshErp, TESTNET_P2SH_P2WSH_ERP, 20),
    ];
    for (network, eras) in [
        (mainnet(), &mainnet_eras[..]),
        (testnet(), &testnet_eras[..]),
    ] {
        let emergency = support::erp_keys_of(&network.0);
        let net = network.2;
        for &(built, block, count) in eras {
            let members = members(count);
            assert_eq!(
                commit(&network, block, &members),
                (Some(1), Some(address_of(built, &members, &emergency, net))),
                "{net}: {built:?} of {count} at #{block}"
            );
        }
    }
}

/// rskj's `buildFederation_with10Members_inMainnet_*`, `with11Members_in
/// Testnet_*`, `with21Members_*` and `with102Members_*`: an ERP federation
/// whose redeem script goes in a scriptSig is built only if the script is
/// within 486 bytes, a scriptSig element less a Flyover prefix
/// (`ScriptValidations.java:11`), which takes nine members with mainnet's four
/// emergency keys and ten with testnet's three; bitcoinj makes no multisig of
/// more than 20 keys (`ScriptBuilder.java:272`). Where the build throws, the
/// commit's deciding vote throws, and nothing is committed.
#[test]
fn a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds() {
    for (network, block, largest, too_many) in [
        (
            mainnet(),
            MAINNET_NON_STANDARD_BEFORE_RSKIP293,
            9,
            &[10][..],
        ),
        (mainnet(), MAINNET_NON_STANDARD, 9, &[10][..]),
        (testnet(), TESTNET_NON_STANDARD, 10, &[11][..]),
        (mainnet(), MAINNET_P2SH_ERP, 9, &[10][..]),
        (testnet(), TESTNET_P2SH_ERP, 10, &[11][..]),
        (mainnet(), MAINNET_P2SH_P2WSH_ERP, 20, &[21, 102][..]),
        (testnet(), TESTNET_P2SH_P2WSH_ERP, 20, &[21][..]),
    ] {
        let net = network.2;
        assert_eq!(
            commit(&network, block, &members(largest)).0,
            Some(1),
            "{net}: {largest} members at #{block}"
        );
        for &count in too_many {
            assert_eq!(
                commit(&network, block, &members(count)),
                (None, None),
                "{net}: {count} members at #{block}"
            );
        }
    }
}

/// rskj's `isComplete_*` and `buildFederation_withLessMembersThanRequired_*`:
/// a pending federation of one member is incomplete, and a vote to commit it
/// answers INSUFFICIENT_MEMBERS (-2) before its hash is looked at
/// (FederationSupportImpl.java:666-669), so none is built.
#[test]
fn a_pending_federation_of_one_member_is_not_committed() {
    let network = mainnet();
    let (config, cfg) = (&network.0, &network.1);
    let mut ctx = support::ctx_at(MAINNET_P2SH_ERP);
    support::pend_federation(&mut ctx, (config, cfg), &members(1));
    let authorizer = support::deciding_authorizers(config)[0];
    let args = encode::args(&[Arg::Bytes(&[0; 32])]);
    let out = governance::commit_federation(
        &mut ctx,
        &args,
        0,
        config,
        cfg,
        &support::sent_by(authorizer),
    )
    .expect("commitFederation answers");
    assert_eq!(support::int_answer(&out.bytes), -2);
}
