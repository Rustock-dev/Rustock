//! rskj's `VoteFederationChangeTest`
//! (`co/rsk/peg/federation/VoteFederationChangeTest.java`): the votes that
//! commit a pending federation, and what the commit writes in each era.
//!
//! Each port runs on mainnet, at a block of the era rskj's activations name,
//! voting as a majority of mainnet's federation-change authorizers. Nothing is
//! stored before it, so the federation committed over is mainnet's genesis
//! federation, where rskj's test stores one of its own.
//!
//! Read in full. Its other tests are of each vote's answers (an unknown
//! function, an unauthorized caller, a vote short of a majority, a key added
//! twice, a rollback), which `bridge/governance.rs`'s tests and the
//! `bridge_federation` ports hold, and of `commitProposedFederation`, which
//! `bridge/governance.rs`'s `the_genesis_federation_leaves_in_its_own_format`
//! reaches.

use crate::bridge::constants::BridgeConstants;
use crate::bridge::storage::{self, BridgeUtxo};
use crate::hardfork::RskHardforkConfig;

use super::support::*;

/// Mainnet blocks of each era: papyrus200's (RSKIP123, not RSKIP186),
/// arrowhead631's (RSKIP186, not RSKIP419), lovell700's (RSKIP419, not
/// RSKIP305) and vetiver900's.
const PAPYRUS200: u64 = 2_500_000;
const ARROWHEAD631: u64 = 6_600_000;
const LOVELL700: u64 = 7_400_000;
const VETIVER900: u64 = 9_000_000;

fn mainnet() -> (BridgeConstants, RskHardforkConfig) {
    (BridgeConstants::mainnet(), RskHardforkConfig::mainnet())
}

/// Mainnet's genesis federation: its keys and its address.
fn genesis() -> (Vec<[u8; 33]>, String) {
    let keys = keys_of(BridgeConstants::mainnet().genesis_federation_public_keys);
    let address = mainnet_p2sh_address(&p2sh_hash(&standard_redeem(&keys)));
    (keys, address)
}

/// Mainnet's emergency keys.
fn emergency() -> Vec<[u8; 33]> {
    keys_of(BridgeConstants::mainnet().erp_fed_pubkeys)
}

/// Two UTXOs of the active federation's, as rskj keeps them.
fn utxos() -> Vec<BridgeUtxo> {
    (1..=2u8)
        .map(|i| BridgeUtxo {
            tx_hash: [i; 32],
            vout: u32::from(i),
            value_satoshis: 100_000_000,
            height: 0,
            script: p2sh_script(&p2sh_hash(&standard_redeem(&genesis().0))),
            coinbase: false,
        })
        .collect()
}

/// A Bridge at `block` of mainnet holding [`utxos`] for the active
/// federation: the UTXO cell's bytes, and the Bridge.
fn holding_utxos(block: u64) -> (Vec<u8>, Ctx) {
    let (_, cfg) = mainnet();
    let mut ctx = ctx_at(block);
    storage::store_federation_utxos(&mut ctx, &cfg, block, &utxos());
    (cell(&mut ctx, storage::NEW_FEDERATION_BTC_UTXOS_KEY), ctx)
}

/// The pending federation's votes are gone (`assertPendingFederationVotingWasCleaned`):
/// no pending federation, and an election with no votes, which rskj writes
/// as an empty list.
fn voting_cleaned(ctx: &mut Ctx) {
    assert!(
        cell(ctx, storage::PENDING_FEDERATION_KEY).is_empty(),
        "no pending federation"
    );
    assert_eq!(
        cell(ctx, storage::FEDERATION_ELECTION_KEY),
        vec![0xc0],
        "no votes"
    );
}

/// rskj `voteCommitFederation_preRSKIP305_commitFederationWithOver10Members_throwsException`:
/// before RSKIP305 (lovell700) the federation a commit builds is a P2SH ERP
/// one, whose redeem script, with a flyover prefix, must fit a scriptSig
/// element; eleven members' don't, and the deciding vote throws. Nothing is
/// committed.
#[test]
fn before_rskip305_a_commit_of_eleven_members_throws() {
    let (config, cfg) = mainnet();
    assert!(cfg.has_rskip419(LOVELL700) && !cfg.has_rskip305(LOVELL700));
    let mut ctx = ctx_at(LOVELL700);
    let answers = elect(&mut ctx, &config, &cfg, &members(11));
    let mut thrown = committed(&config);
    *thrown.last_mut().expect("a vote") = None;
    assert_eq!(answers, thrown);
    assert!(
        cell(&mut ctx, PROPOSED.0).is_empty(),
        "no proposed federation"
    );
    assert!(
        !cell(&mut ctx, storage::PENDING_FEDERATION_KEY).is_empty(),
        "still pending"
    );
}

/// rskj `voteCommitFederation_postRSKIP305_commitFederationWith20Members_shouldNotThrow`:
/// from RSKIP305 a commit builds a P2SH-P2WSH ERP federation, whose script
/// goes in the witness: twenty members commit, as a proposed federation of
/// twenty.
#[test]
fn from_rskip305_a_commit_of_twenty_members_is_proposed() {
    let (config, cfg) = mainnet();
    let keys = members(20);
    let mut ctx = ctx_at(VETIVER900);
    assert_eq!(elect(&mut ctx, &config, &cfg, &keys), committed(&config));
    assert_eq!(
        cell(&mut ctx, PROPOSED.0),
        serialize_federation(&keys, TIMESTAMP * 1000, VETIVER900)
    );
    assert_eq!(cell(&mut ctx, PROPOSED.1), rlp_number(4000));
}

/// rskj `voteCommitFederation_preRSKIP186_whenPendingFederationIsSet_shouldPerformLegacyCommitFederationActionsButNotSetFederationChangeInfo`:
/// before RSKIP186 (papyrus200) a commit hands over at once: the federation
/// built from the pending one, created at the block with its timestamp read
/// as milliseconds, is the new federation, the active one the old, the
/// active one's UTXOs the old one's; the votes are cleaned and
/// `commit_federation` logged. No creation height and no retired script are
/// kept.
#[test]
fn before_rskip186_a_commit_hands_over_without_the_federation_change_information() {
    let (config, cfg) = mainnet();
    assert!(cfg.has_rskip123(PAPYRUS200) && !cfg.has_rskip186(PAPYRUS200));
    let keys = members(9);
    let (genesis_keys, genesis_address) = genesis();
    let (held, mut ctx) = holding_utxos(PAPYRUS200);

    assert_eq!(elect(&mut ctx, &config, &cfg, &keys), committed(&config));
    assert_eq!(
        logs(&mut ctx),
        vec![commit_federation_event(
            (&genesis_keys, &genesis_address),
            (
                &keys,
                &mainnet_p2sh_address(&p2sh_hash(&standard_redeem(&keys)))
            ),
            PAPYRUS200 + 18_500,
        )]
    );
    assert_eq!(
        cell(&mut ctx, NEW.0),
        serialize_federation(&keys, TIMESTAMP, PAPYRUS200)
    );
    assert_eq!(cell(&mut ctx, NEW.1), rlp_number(1000));
    assert_eq!(
        cell(&mut ctx, OLD.0),
        serialize_federation(&genesis_keys, MAINNET_GENESIS_CREATION_MILLIS, 1)
    );
    assert_eq!(cell(&mut ctx, OLD.1), rlp_number(1000));
    assert_eq!(cell(&mut ctx, storage::OLD_FEDERATION_BTC_UTXOS_KEY), held);
    assert_eq!(
        cell(&mut ctx, storage::NEW_FEDERATION_BTC_UTXOS_KEY),
        vec![0xc0]
    );
    for unset in [
        storage::ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        storage::LAST_RETIRED_FEDERATION_P2SH_SCRIPT_KEY,
    ] {
        assert!(cell(&mut ctx, unset).is_empty(), "{unset}");
    }
    voting_cleaned(&mut ctx);
}

/// rskj `voteCommitFederation_postRSKIP186_preRSKIP419_whenPendingFederationIsSet_shouldPerformLegacyCommitFederationActions`:
/// from RSKIP186 to RSKIP419 (arrowhead631) the handover also keeps the
/// retiring federation's P2SH script and the new federation's creation
/// block as the next one; the federation built is a P2SH ERP one.
#[test]
fn from_rskip186_until_rskip419_a_commit_hands_over_with_the_federation_change_information() {
    let (config, cfg) = mainnet();
    assert!(cfg.has_rskip186(ARROWHEAD631) && !cfg.has_rskip419(ARROWHEAD631));
    let keys = members(9);
    let (genesis_keys, genesis_address) = genesis();
    let (held, mut ctx) = holding_utxos(ARROWHEAD631);

    assert_eq!(elect(&mut ctx, &config, &cfg, &keys), committed(&config));
    let redeem = p2sh_erp_redeem(&keys, &emergency(), 52_560);
    assert_eq!(
        logs(&mut ctx),
        vec![commit_federation_event(
            (&genesis_keys, &genesis_address),
            (&keys, &mainnet_p2sh_address(&p2sh_hash(&redeem))),
            ARROWHEAD631 + 40_320,
        )]
    );
    assert_eq!(
        cell(&mut ctx, NEW.0),
        serialize_federation(&keys, TIMESTAMP, ARROWHEAD631)
    );
    assert_eq!(cell(&mut ctx, NEW.1), rlp_number(3000));
    assert_eq!(
        cell(&mut ctx, OLD.0),
        serialize_federation(&genesis_keys, MAINNET_GENESIS_CREATION_MILLIS, 1)
    );
    assert_eq!(cell(&mut ctx, storage::OLD_FEDERATION_BTC_UTXOS_KEY), held);
    assert_eq!(
        cell(&mut ctx, storage::NEW_FEDERATION_BTC_UTXOS_KEY),
        vec![0xc0]
    );
    assert_eq!(
        cell(&mut ctx, storage::LAST_RETIRED_FEDERATION_P2SH_SCRIPT_KEY),
        serialize_script(&p2sh_script(&p2sh_hash(&standard_redeem(&genesis_keys))))
    );
    assert_eq!(
        cell(&mut ctx, storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY),
        rlp_number(ARROWHEAD631)
    );
    assert!(
        cell(&mut ctx, PROPOSED.0).is_empty(),
        "no proposed federation"
    );
    voting_cleaned(&mut ctx);
}

/// rskj `voteCommitFederation_postRSKIP419_whenPendingFederationIsSet_shouldPerformCommitFederationActions`:
/// from RSKIP419 a commit proposes the federation, created at the block with
/// its timestamp in seconds, and hands nothing over: the federations and
/// their UTXOs stay as they were. The votes are cleaned and
/// `commit_federation` logged.
#[test]
fn from_rskip419_a_commit_proposes_the_federation_and_hands_nothing_over() {
    let (config, cfg) = mainnet();
    let keys = members(9);
    let (genesis_keys, genesis_address) = genesis();
    let (held, mut ctx) = holding_utxos(VETIVER900);

    assert_eq!(elect(&mut ctx, &config, &cfg, &keys), committed(&config));
    let redeem = p2sh_erp_redeem(&keys, &emergency(), 52_560);
    assert_eq!(
        logs(&mut ctx),
        vec![commit_federation_event(
            (&genesis_keys, &genesis_address),
            (&keys, &mainnet_p2sh_address(&p2sh_p2wsh_hash(&redeem))),
            VETIVER900 + 40_320,
        )]
    );
    assert_eq!(
        cell(&mut ctx, PROPOSED.0),
        serialize_federation(&keys, TIMESTAMP * 1000, VETIVER900)
    );
    assert_eq!(cell(&mut ctx, PROPOSED.1), rlp_number(4000));
    for untouched in [
        NEW.0,
        NEW.1,
        OLD.0,
        OLD.1,
        storage::OLD_FEDERATION_BTC_UTXOS_KEY,
    ] {
        assert!(cell(&mut ctx, untouched).is_empty(), "{untouched}");
    }
    assert_eq!(cell(&mut ctx, storage::NEW_FEDERATION_BTC_UTXOS_KEY), held);
    voting_cleaned(&mut ctx);
}
