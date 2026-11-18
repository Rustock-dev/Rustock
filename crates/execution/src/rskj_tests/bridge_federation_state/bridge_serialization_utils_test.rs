//! rskj's `BridgeSerializationUtilsTest`
//! (`co/rsk/peg/BridgeSerializationUtilsTest.java`), its listed tests of a
//! non-standard ERP federation (format 2000) written and read back: the same
//! federation, its redeem script built anew with the activations it is read
//! under.
//!
//! rskj reads a stored federation with the block's activations, so each port
//! keeps the federation as rskj writes it (`serializeFederation`, format 2000)
//! as the only one, at a block of the era rskj's test names, and reads back
//! its members, creation time and block, and the redeem script the Bridge
//! takes for it. Each era's script is the `bridge_federation` ports'
//! (`non_standard_erp_federations_test.rs`), here read through the store.
//!
//! Read in full. Its other tests are of other values' encodings (votes,
//! locks, peg-outs, UTXOs, coinbase information), each held where the value
//! is used.

use crate::bridge::constants::BridgeConstants;
use crate::bridge::{federation, peg, storage};
use crate::hardfork::RskHardforkConfig;

use super::support::*;

/// rskj's `TestConstants.NON_STANDARD_ERP_REDEEM_SCRIPT_HARDCODED`: testnet's
/// non-standard ERP redeem script before RSKIP284, whatever the members.
const TESTNET_HARDCODED: &str = "6453210208f40073a9e43b3e9103acec79767a6de9b0409749884e989960fee578012fce210225e892391625854128c5c4ea4340de0c2a70570f33db53426fc9c746597a03f42102afc230c2d355b1a577682b07bc2646041b5d0177af0f98395a46018da699b6da210344a3c38cd59afcba3edcebe143e025574594b001700dec41e59409bdbd0f2a0921039a060badbeb24bee49eb2063f616c0f0f0765d4ca646b20a88ce828f259fcdb955670300cd50b27552210216c23b2ea8e4f11c3f9e22711addb1d16a93964796913830856b568cc3ea21d3210275562901dd8faae20de0a4166362a4f82188db77dbed4ca887422ea1ec185f1421034db69f2112f4fb1bb6141bf6e2bd6631f0484d0bd95b16767902c9fe219d4a6f5368ae";

/// 52,560, the emergency delay of mainnet and testnet, as each builder
/// pushes it: before RSKIP293 in two big-endian bytes, from it as
/// `signedLongToByteArrayLE` writes it.
const DELAY_BEFORE_RSKIP293: &[u8] = &[0xcd, 0x50];
const DELAY_FROM_RSKIP293: &[u8] = &[0x50, 0xcd, 0x00];

/// The federation, kept as rskj keeps it with format 2000 at `block` of
/// `network`, read back: whether it reads as written, and the redeem script
/// the Bridge takes for it.
fn written_and_read(network: (BridgeConstants, RskHardforkConfig), block: u64) -> Vec<u8> {
    let (config, cfg) = network;
    let keys = members(9);
    let mut ctx = ctx_at(block);
    store_federation(&mut ctx, NEW, &keys, 1_600_000_000, block - 100, Some(2000));
    let read = federation::load_stored_federation(&mut ctx, storage::NEW_FEDERATION_KEY)
        .expect("the federation reads back");
    let mut read_keys = read.btc_keys();
    read_keys.sort();
    assert_eq!(read_keys, keys);
    assert_eq!(
        (read.creation_time_millis, read.creation_block),
        (1_600_000_000, block - 100)
    );
    peg::active_federation_keys_and_redeem(&mut ctx, &config, &cfg, block).1
}

fn mainnet() -> (BridgeConstants, RskHardforkConfig) {
    (BridgeConstants::mainnet(), RskHardforkConfig::mainnet())
}

fn testnet() -> (BridgeConstants, RskHardforkConfig) {
    (BridgeConstants::testnet(), RskHardforkConfig::testnet())
}

/// The non-standard ERP script of the members with `config`'s emergency keys.
fn era_script(config: &BridgeConstants, delay: &[u8]) -> Vec<u8> {
    non_standard_erp_redeem(&members(9), &keys_of(config.erp_fed_pubkeys), delay)
}

/// rskj `serializeAndDeserializeNonStandardErpFederation_preRSKIP284_testnet`:
/// testnet from RSKIP201 (iris300) to RSKIP284 reads the federation back with
/// its hardcoded script.
#[test]
fn on_testnet_before_rskip284_the_federation_reads_back_with_the_hardcoded_script() {
    let block = 2_500_000;
    let cfg = RskHardforkConfig::testnet();
    assert!(cfg.has_rskip201(block) && !cfg.has_rskip284(block));
    assert_eq!(
        written_and_read(testnet(), block),
        hex::decode(TESTNET_HARDCODED).expect("hex")
    );
}

/// rskj `serializeAndDeserializeNonStandardErpFederation_postRSKIP284_preRSKIP293_testnet`:
/// testnet from RSKIP284 to RSKIP293 (2,581,800 to 3,103,000) reads it back
/// with its own script, the delay in two big-endian bytes, not the
/// hardcoded one.
#[test]
fn on_testnet_from_rskip284_until_rskip293_the_federation_reads_back_with_its_own_script() {
    let block = 2_600_000;
    let cfg = RskHardforkConfig::testnet();
    assert!(cfg.has_rskip284(block) && !cfg.has_rskip293(block));
    let script = written_and_read(testnet(), block);
    assert_eq!(
        script,
        era_script(&BridgeConstants::testnet(), DELAY_BEFORE_RSKIP293)
    );
    assert_ne!(script, hex::decode(TESTNET_HARDCODED).expect("hex"));
}

/// rskj `serializeAndDeserializeNonStandardErpFederation_preRSKIP293_mainnet`:
/// mainnet from RSKIP201 to RSKIP293 (iris300 to hop400) reads it back with
/// its own script, the delay in two big-endian bytes.
#[test]
fn on_mainnet_before_rskip293_the_federation_reads_back_with_its_own_script() {
    let block = 4_000_000;
    let cfg = RskHardforkConfig::mainnet();
    assert!(cfg.has_rskip201(block) && !cfg.has_rskip293(block));
    assert_eq!(
        written_and_read(mainnet(), block),
        era_script(&BridgeConstants::mainnet(), DELAY_BEFORE_RSKIP293)
    );
}

/// rskj `serializeAndDeserializeNonStandardErpFederation_postRSKIP293_testnet`:
/// from RSKIP293 testnet pushes the delay as a Java number.
#[test]
fn on_testnet_from_rskip293_the_federation_reads_back_with_the_delay_as_a_java_number() {
    let block = 3_200_000;
    assert!(RskHardforkConfig::testnet().has_rskip293(block));
    assert_eq!(
        written_and_read(testnet(), block),
        era_script(&BridgeConstants::testnet(), DELAY_FROM_RSKIP293)
    );
}

/// rskj `serializeAndDeserializeNonStandardErpFederation_postRSKIP293_mainnet`:
/// from RSKIP293 (hop400) mainnet pushes it as a Java number too.
#[test]
fn on_mainnet_from_rskip293_the_federation_reads_back_with_the_delay_as_a_java_number() {
    let block = 4_700_000;
    assert!(RskHardforkConfig::mainnet().has_rskip293(block));
    assert_eq!(
        written_and_read(mainnet(), block),
        era_script(&BridgeConstants::mainnet(), DELAY_FROM_RSKIP293)
    );
}
