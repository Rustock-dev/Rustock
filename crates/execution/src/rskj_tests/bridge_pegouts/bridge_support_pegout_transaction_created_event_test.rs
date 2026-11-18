//! rskj's `BridgeSupportPegoutTransactionCreatedEventTest`: from RSKIP428
//! (lovell700) each peg-out `updateCollections` builds, a batch or a
//! migration, is logged as `pegout_transaction_created`, its hash a topic and
//! its data the values of the outputs it spends, each a Bitcoin compact size
//! (`UtxoUtils.encodeOutpointValues`). rskj's tests run under arrowhead600
//! and lovell700; the ports run mainnet's Bridge at a block of each, its
//! UTXOs a bitcoin each.

use super::support::*;

const ERAS: [(u64, bool); 2] = [(MAINNET_ARROWHEAD600, false), (MAINNET_LOVELL700, true)];

/// A bitcoin as a Bitcoin compact size.
const A_BITCOIN: [u8; 5] = [0xfe, 0x00, 0xe1, 0xf5, 0x05];

/// The `pegout_transaction_created` events among `logged`: the hash topic,
/// and the bytes of the data.
fn created(logged: &[revm::primitives::Log]) -> Vec<([u8; 32], Vec<u8>)> {
    let topic = event_topic("pegout_transaction_created(bytes32,bytes)");
    logged
        .iter()
        .filter(|log| topics(log)[0] == topic)
        .map(|log| {
            let data = &log.data.data;
            let length = word(data, 1).to::<usize>();
            (topics(log)[1], data[64..64 + length].to_vec())
        })
        .collect()
}

/// What rskj logs of `tx`: its hash as bitcoinj's `Sha256Hash` holds it,
/// and the value of each output it spends.
fn event_of(tx: &bitcoin::Transaction) -> ([u8; 32], Vec<u8>) {
    (reversed(txid(tx)), A_BITCOIN.repeat(tx.input.len()))
}

/// How many logs among `logged` are of the event `signature`.
fn count(logged: &[revm::primitives::Log], signature: &str) -> usize {
    let topic = event_topic(signature);
    logged.iter().filter(|log| topics(log)[0] == topic).count()
}

/// rskj's `updateCollections_whenPegoutBatchIsCreated_shouldLogPegoutTransactionCreatedEvent`:
/// three requests make a batch, logged as `batch_pegout_created` and
/// `release_requested`, and from RSKIP428 as `pegout_transaction_created`.
#[test]
fn from_rskip428_a_pegout_batch_built_is_logged_with_the_values_it_spends() {
    for (number, rskip428) in ERAS {
        let mut collections = Collections::mainnet_at(number);
        collections.active(1, 10);
        collections.requests(3);
        let logged = collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        assert_eq!(
            count(&logged, "batch_pegout_created(bytes32,bytes)"),
            1,
            "{number}"
        );
        assert_eq!(
            count(&logged, "release_requested(bytes32,bytes32,uint256)"),
            1,
            "{number}"
        );
        let expected = if rskip428 {
            vec![event_of(&waiting[0])]
        } else {
            vec![]
        };
        assert_eq!(created(&logged), expected, "{number}");
    }
}

/// rskj's `updateCollections_whenPegoutMigrationIsCreated_shouldLogPegoutTransactionCreatedEvent`:
/// a migration, logged as `release_requested`, and from RSKIP428 as
/// `pegout_transaction_created`.
#[test]
fn from_rskip428_a_migration_built_is_logged_with_the_values_it_spends() {
    for (number, rskip428) in ERAS {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, 10, false);
        let logged = collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        assert_eq!(
            count(&logged, "release_requested(bytes32,bytes32,uint256)"),
            1,
            "{number}"
        );
        let expected = if rskip428 {
            vec![event_of(&waiting[0])]
        } else {
            vec![]
        };
        assert_eq!(created(&logged), expected, "{number}");
    }
}

/// rskj's `updateCollections_whenPegoutMigrationAndBatchAreCreated_shouldLogPegoutTransactionCreatedEvent`:
/// a migration and a batch built in one call, each logged from RSKIP428.
#[test]
fn from_rskip428_a_migration_and_a_pegout_batch_built_together_are_each_logged() {
    for (number, rskip428) in ERAS {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, 10, false);
        collections.requests(3);
        let logged = collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 2, "{number}");
        let mut expected: Vec<_> = if rskip428 {
            waiting.iter().map(event_of).collect()
        } else {
            vec![]
        };
        let mut logged = created(&logged);
        expected.sort();
        logged.sort();
        assert_eq!(logged, expected, "{number}");
    }
}
