//! rskj's `ReleaseTransactionBuilderTest`: here its tests of the Bitcoin
//! version of what it builds, 1 before RSKIP201 (iris300) for a peg-out
//! (`buildAmountTo`) and a refund (`buildEmptyWalletTo`), and 1 before
//! RSKIP376 (arrowhead600) for a migration (`buildMigrationTransaction`); 2
//! from them. rskj's tests call the builder; the ports have the Bridge build
//! each, a block before the rule and at it, and read the version.

use super::support::*;

/// The version of each peg-out waiting for confirmations in `ctx`.
fn versions(ctx: &mut Ctx) -> Vec<i32> {
    crate::bridge::peg::load_pegout_confirmation_set(ctx, true)
        .iter()
        .map(|entry| {
            let tx: bitcoin::Transaction =
                bitcoin::consensus::deserialize(&entry.btc_tx_raw).expect("a transaction");
            tx.version.0
        })
        .collect()
}

/// rskj's `BuildAmountToTest.buildAmountTo_whenRSKIP201IsNotActive_shouldCreatePegoutTxWithBtcVersion1`.
#[test]
fn before_rskip201_a_pegout_is_built_as_version_1() {
    for (number, version) in [(MAINNET_IRIS300 - 1, 1), (MAINNET_IRIS300, 2)] {
        let mut collections = Collections::mainnet_at(number);
        collections.active(1, 10);
        collections.requests(1);
        collections.update();
        assert_eq!(
            versions(&mut collections.bridge.ctx),
            vec![version],
            "{number}"
        );
    }
}

/// rskj's `BuildEmptyWalletToTest.StandardMultiSigFederationTest.buildEmptyWalletTo_whenRSKIP201IsNotActive_shouldCreateRefundTxWithBtcVersion1`:
/// a peg-in from a multisig sender, refunded on rskj's regtest under
/// papyrus200 and under iris300.
#[test]
fn before_rskip201_a_refund_is_built_as_version_1() {
    let sender = standard_redeem(&keys(20, 3));
    let tx = btc_transaction(
        vec![input(1, multisig_spend(&sender))],
        &[(BTC, address_of(&active()))],
    );
    for (last, version) in [
        (RskNetworkUpgrade::Papyrus200, 1),
        (RskNetworkUpgrade::Iris300, 2),
    ] {
        let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
        let mut bridge = Registration::new(cfg, BridgeConstants::rskj_regtest(), 1_000);
        bridge.federation(NEW_FEDERATION, &active());
        let outcome = bridge.register(&tx, 1_000);
        assert_eq!(outcome.refunds, 1, "{last:?}");
        assert_eq!(versions(&mut bridge.ctx), vec![version], "{last:?}");
    }
}

/// rskj's `BuildMigrationTransactionTest.StandardMultiSigFederationTests.buildMigrationTransaction_whenRSKIP376IsNotActive_shouldCreateMigrationTxWithBtcVersion1`
/// and `BuildMigrationTransactionTest.P2shErpFederationTests.buildMigrationTransaction_whenRSKIP376IsNotActive_shouldCreateMigrationTxWithBtcVersion1`:
/// a migration from a retiring federation stored as a standard multisig
/// one and as a P2SH ERP one.
#[test]
fn before_rskip376_a_migration_is_built_as_version_1() {
    for format in [STANDARD_MULTISIG, P2SH_ERP] {
        for (number, version) in [(MAINNET_ARROWHEAD600 - 1, 1), (MAINNET_ARROWHEAD600, 2)] {
            let mut collections = Collections::migrating(number, format, 10, false);
            collections.update();
            assert_eq!(
                versions(&mut collections.bridge.ctx),
                vec![version],
                "{format} {number}"
            );
        }
    }
}
