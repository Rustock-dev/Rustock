//! rskj's `FederationStorageProviderImplTests`
//! (`co/rsk/peg/federation/FederationStorageProviderImplTests.java`): how the
//! Bridge keeps its federations and their UTXOs.
//!
//! rskj's tests set a federation on the provider and save it; here the
//! federation is put where rskj's own code puts it, by a commit or by the
//! end of a funds migration, and the cells are read after it.
//!
//! Read in full. Its other tests read back what was stored (each format, a
//! cached value, the pending federation, the election, the creation heights,
//! the last retired script), which the ports of `FederationSupportImplTest`
//! and `bridge/governance.rs`'s tests reach through the methods that read
//! them.

use crate::bridge::constants::BridgeConstants;
use crate::bridge::peg;
use crate::bridge::storage::{self, BridgeUtxo};
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::BridgeTxContext;

use super::support::*;

/// Mainnet blocks of each era: before RSKIP123, papyrus200's (standard
/// multisig), iris300's (non-standard ERP) and hop401's (P2SH ERP).
const ERAS: [(u64, Option<u64>); 4] = [
    (1_000_000, None),
    (2_500_000, Some(1000)),
    (4_000_000, Some(2000)),
    (5_000_000, Some(3000)),
];

/// A federation of nine committed at `block` of mainnet, with nothing
/// stored before it.
fn committed_at(block: u64) -> Ctx {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let mut ctx = ctx_at(block);
    assert_eq!(
        elect(&mut ctx, &config, &cfg, &members(9)),
        committed(&config)
    );
    ctx
}

/// rskj `testSaveNewFederation`: the new federation is kept with all its
/// members' keys and its format version from RSKIP123, and with its members'
/// BTC keys alone and no format before it, in each era's format.
#[test]
fn a_committed_federation_is_kept_as_the_new_one_as_rskj_keeps_it_in_each_era() {
    let cfg = RskHardforkConfig::mainnet();
    for (block, format) in ERAS {
        assert_eq!(cfg.has_rskip123(block), format.is_some());
        let mut ctx = committed_at(block);
        let keys = members(9);
        match format {
            Some(format) => {
                assert_eq!(
                    cell(&mut ctx, NEW.0),
                    serialize_federation(&keys, TIMESTAMP, block)
                );
                assert_eq!(cell(&mut ctx, NEW.1), rlp_number(format), "at {block}");
            }
            None => {
                assert_eq!(
                    cell(&mut ctx, NEW.0),
                    serialize_federation_only_btc_keys(&keys, TIMESTAMP, block)
                );
                assert!(
                    cell(&mut ctx, NEW.1).is_empty(),
                    "no format before RSKIP123"
                );
            }
        }
    }
}

/// rskj `testSaveOldFederation`: the federation committed over, here
/// mainnet's genesis one, is kept as the old one, as the new one is.
#[test]
fn the_federation_committed_over_is_kept_as_the_old_one_as_rskj_keeps_it_in_each_era() {
    let genesis = keys_of(BridgeConstants::mainnet().genesis_federation_public_keys);
    for (block, format) in ERAS {
        let mut ctx = committed_at(block);
        match format {
            Some(_) => {
                assert_eq!(
                    cell(&mut ctx, OLD.0),
                    serialize_federation(&genesis, MAINNET_GENESIS_CREATION_MILLIS, 1)
                );
                assert_eq!(cell(&mut ctx, OLD.1), rlp_number(1000), "at {block}");
            }
            None => {
                assert_eq!(
                    cell(&mut ctx, OLD.0),
                    serialize_federation_only_btc_keys(
                        &genesis,
                        MAINNET_GENESIS_CREATION_MILLIS,
                        1
                    )
                );
                assert!(
                    cell(&mut ctx, OLD.1).is_empty(),
                    "no format before RSKIP123"
                );
            }
        }
    }
}

/// rskj `saveProposedFederation_preRSKIP419_whenProposedFederationIsSet_shouldNotSave`:
/// before RSKIP419 (arrowhead631) a commit keeps no proposed federation.
#[test]
fn before_rskip419_no_proposed_federation_is_kept() {
    let block = 6_600_000;
    assert!(!RskHardforkConfig::mainnet().has_rskip419(block));
    let mut ctx = committed_at(block);
    assert!(!cell(&mut ctx, NEW.0).is_empty());
    assert!(cell(&mut ctx, PROPOSED.0).is_empty() && cell(&mut ctx, PROPOSED.1).is_empty());
}

/// A mainnet Bridge at `block`, whose new federation of nine was created at
/// `created` in `format` (or before RSKIP123, with `None`), with an old
/// federation of three kept the same way, after `updateCollections`.
fn retired_at(block: u64, created: u64, format: Option<u64>) -> Ctx {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let mut ctx = ctx_at(block);
    let old: Vec<[u8; 33]> = (0..3u8).map(|i| public_key(&[0x61 + i])).collect();
    store_federation(&mut ctx, NEW, &members(9), 1_000, created, format);
    store_federation(
        &mut ctx,
        OLD,
        &old,
        500,
        created - 1_000,
        format.map(|_| 1000),
    );
    peg::update_collections(&mut ctx, 0, &config, &cfg, &BridgeTxContext::default())
        .expect("updateCollections returns");
    ctx
}

/// rskj `saveOldFederation_before_RSKIP123_should_save_null`: before RSKIP123,
/// the end of the funds migration clears the old federation's cell and
/// writes no format.
#[test]
fn before_rskip123_the_retired_federation_is_cleared_without_a_format() {
    let created = 1_000_000;
    let end = created + 18_500 + 10_585;
    assert!(!RskHardforkConfig::mainnet().has_rskip123(end));
    let mut ctx = retired_at(end - 1, created, None);
    assert!(!cell(&mut ctx, OLD.0).is_empty(), "migrating, still kept");
    let mut ctx = retired_at(end, created, None);
    assert!(cell(&mut ctx, OLD.0).is_empty(), "cleared");
    assert!(cell(&mut ctx, OLD.1).is_empty(), "no format");
}

/// rskj `saveOldFederation_after_RSKIP123_should_save_null`: from RSKIP123,
/// clearing the old federation also writes its format cell, as a standard
/// multisig's (1000), which rskj takes a federation of none to be. The
/// federations here were kept before RSKIP123, without format cells, and the
/// migration ends after it.
#[test]
fn from_rskip123_the_retired_federation_is_cleared_with_a_standard_format() {
    let created = 1_570_000;
    let end = created + 18_500 + 10_585;
    let cfg = RskHardforkConfig::mainnet();
    assert!(!cfg.has_rskip123(created) && cfg.has_rskip123(end));
    let mut ctx = retired_at(end, created, None);
    assert!(cell(&mut ctx, OLD.0).is_empty(), "cleared");
    assert_eq!(cell(&mut ctx, OLD.1), rlp_number(1000));
}

/// Three UTXOs of a P2PKH output, as rskj's `UTXOBuilder` makes them.
fn utxos(count: u8) -> Vec<BridgeUtxo> {
    (1..=count)
        .map(|i| BridgeUtxo {
            tx_hash: [i; 32],
            vout: 0,
            value_satoshis: 100_000_000,
            height: 0,
            script: [&[0x76, 0xa9, 20][..], &[0x44; 20], &[0x88, 0xac]].concat(),
            coinbase: false,
        })
        .collect()
}

/// `serializeUTXOList`: each UTXO as bitcoinj-thin's `UTXO.serializeToStream`
/// writes it (its value, its script's length in four bytes and its script,
/// its transaction's hash, its index, its height and whether it is a
/// coinbase's), as an element of a list.
fn serialize_utxo_list(utxos: &[BridgeUtxo]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = utxos
        .iter()
        .map(|utxo| {
            let mut bytes = utxo.value_satoshis.to_le_bytes().to_vec();
            bytes.extend_from_slice(&(utxo.script.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&utxo.script);
            bytes.extend_from_slice(&utxo.tx_hash);
            bytes.extend_from_slice(&utxo.vout.to_le_bytes());
            bytes.extend_from_slice(&utxo.height.to_le_bytes());
            bytes.push(u8::from(utxo.coinbase));
            rlp_element(&bytes)
        })
        .collect();
    rlp_list(&items)
}

/// The UTXO cells of `provideFederationBtcUTXOsTestArguments`: testnet's
/// general cell before RSKIP284, its pre-hop cell from it to RSKIP293 (hop400)
/// and its post-hop cell from that; mainnet's general cell throughout.
fn utxo_cells() -> [(RskHardforkConfig, u64, &'static str); 5] {
    [
        (
            RskHardforkConfig::testnet(),
            2_581_799,
            storage::NEW_FEDERATION_BTC_UTXOS_KEY,
        ),
        (
            RskHardforkConfig::testnet(),
            2_581_800,
            storage::NEW_FEDERATION_BTC_UTXOS_TESTNET_PRE_HOP_KEY,
        ),
        (
            RskHardforkConfig::testnet(),
            3_103_000,
            storage::NEW_FEDERATION_BTC_UTXOS_TESTNET_POST_HOP_KEY,
        ),
        (
            RskHardforkConfig::mainnet(),
            2_581_800,
            storage::NEW_FEDERATION_BTC_UTXOS_KEY,
        ),
        (
            RskHardforkConfig::mainnet(),
            4_598_500,
            storage::NEW_FEDERATION_BTC_UTXOS_KEY,
        ),
    ]
}

const UTXO_CELLS: [&str; 3] = [
    storage::NEW_FEDERATION_BTC_UTXOS_KEY,
    storage::NEW_FEDERATION_BTC_UTXOS_TESTNET_PRE_HOP_KEY,
    storage::NEW_FEDERATION_BTC_UTXOS_TESTNET_POST_HOP_KEY,
];

/// rskj `getNewFederationBtcUTXOs`: the active federation's UTXOs are read
/// from the cell of the network and the era, and from no other.
#[test]
fn the_active_federations_utxos_are_read_from_the_cell_of_the_network_and_era() {
    for (cfg, block, key) in utxo_cells() {
        let mut ctx = ctx_at(block);
        put(&mut ctx, key, &serialize_utxo_list(&utxos(2)));
        let read = storage::load_federation_utxos(&mut ctx, &cfg, block);
        assert_eq!(
            serialize_utxo_list(&read),
            serialize_utxo_list(&utxos(2)),
            "{key} at {block}"
        );
        for other in UTXO_CELLS.into_iter().filter(|other| *other != key) {
            let mut ctx = ctx_at(block);
            put(&mut ctx, other, &serialize_utxo_list(&utxos(2)));
            assert!(
                storage::load_federation_utxos(&mut ctx, &cfg, block).is_empty(),
                "{other} at {block}"
            );
        }
    }
}

/// rskj `saveNewFederationBtcUTXOs_utxosShouldBeSavedToStorage`: UTXOs added
/// to an empty set are written, as rskj writes them, in the cell of the
/// network and the era, and read back.
#[test]
fn the_active_federations_utxos_are_written_in_the_cell_of_the_network_and_era() {
    for (cfg, block, key) in utxo_cells() {
        let mut ctx = ctx_at(block);
        put(&mut ctx, key, &[]);
        let mut held = storage::load_federation_utxos(&mut ctx, &cfg, block);
        assert!(held.is_empty());
        held.extend(utxos(3));
        storage::store_federation_utxos(&mut ctx, &cfg, block, &held);
        assert_eq!(
            cell(&mut ctx, key),
            serialize_utxo_list(&utxos(3)),
            "{key} at {block}"
        );
        for other in UTXO_CELLS.into_iter().filter(|other| *other != key) {
            assert!(cell(&mut ctx, other).is_empty(), "{other} at {block}");
        }
        let read = storage::load_federation_utxos(&mut ctx, &cfg, block);
        assert_eq!(serialize_utxo_list(&read), serialize_utxo_list(&utxos(3)));
    }
}
