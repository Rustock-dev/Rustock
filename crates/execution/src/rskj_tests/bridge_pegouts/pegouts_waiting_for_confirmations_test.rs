//! rskj's `PegoutsWaitingForConfirmationsTest`, and the set `updateCollections`
//! walks for the next peg-out with its confirmations. Before RSKIP559, which
//! is `tbd1000` in the release's `reference.conf` and so on no network,
//! `getNextPegoutWithEnoughConfirmations` takes the first confirmed peg-out in
//! the order a Java `HashSet` walks its table: bucket by bucket, in a table
//! sized when `BridgeStorageProvider` read the set and grown by what the same
//! call added to it.

use super::support::*;
use crate::bridge::peg::{self, PegoutWaitingForConfirmations};

/// The transaction rskj's test builds: one spending output 0 of a
/// transaction that pays fifty bitcoins to the key of the private key
/// 123456, its input disconnected, and paying `value` to the key of the
/// private key `to`.
fn paying(to: u8, value: u64) -> bitcoin::Transaction {
    let funding = btc_transaction(
        vec![],
        &[(50 * BTC, p2pkh(&hash160(&public_key(&[0x01, 0xe2, 0x40]))))],
    );
    let spend = bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: funding.compute_txid(),
            vout: 0,
        },
        ..input(0, vec![])
    };
    btc_transaction(vec![spend], &[(value, p2pkh(&hash160(&public_key(&[to]))))])
}

/// `getNextPegoutWithEnoughConfirmation_multipleMatch_rskip559Off`: of the
/// eight peg-outs rskj's test keeps, the four made at block 5 have their five
/// confirmations at block 10, and the one taken is the first of them the
/// set's table of 16 walks to, `53efc6f7…`.
#[test]
fn before_rskip559_the_next_pegout_is_the_first_confirmed_one_the_set_walks_to() {
    let entries: Vec<PegoutWaitingForConfirmations> = [
        (2, 150, 32),
        (5, BTC, 100),
        (4, 50 * BTC, 7),
        (3, BTC / 1000, 10),
        (8, 5 * (BTC / 100), 5),
        (11, BTC / 1000, 5),
        (12, BTC / 1000, 5),
        (13, BTC / 1000, 5),
    ]
    .into_iter()
    .map(|(to, value, height)| PegoutWaitingForConfirmations {
        btc_tx_raw: bitcoin::consensus::serialize(&paying(to, value)),
        rsk_block_height: height,
        rsk_tx_hash: None,
    })
    .collect();
    let next = peg::next_pegout_with_enough_confirmations(&entries, 10, 5, entries.len())
        .expect("a peg-out with its confirmations");
    let taken: bitcoin::Transaction =
        bitcoin::consensus::deserialize(&entries[next].btc_tx_raw).expect("a transaction");
    assert_eq!(
        taken.compute_txid().to_string(),
        "53efc6f78eb9d159cfee76ec45bcffb08fd11f85c762e1eacf54e5c014da219d"
    );
}

/// Mainnet's two peg-outs made at block #3,340,302, both confirmed at
/// #3,344,303. This one, `20806dad…`, is in bucket 15 of a table of 32 and
/// the other in bucket 21; rskj took this one at #3,344,303, from a set of
/// twelve it read.
const FIRST_OF_32_BUCKETS: &str = "010000000158fc2de209f41b6faa714856b621aec5b268369804046269e017fc7107ead5ca01000000fdc80100000000000000004dbd0157210231a395e332dde8688800a0025cccc5771ea1aa874a633b8ab6e5c89d300c7c3621026b472f7d59d201ff1f540f111b6eb329e071c30a9d23e3d2bcd128fe73dc254c21027319afb15481dbeb3c426bcc37f9a30e7f51ceff586936d85548d9395bcc2344210294c817150f78607566e961b3c71df53a22022a80acbb982f83c0c8baac040adc2103250c11be0561b1d7ae168b1f59e39cbc1fd1ba3cf4d2140c1a365b2723a2bf9321033ada6ef3b1d93a1978b595c7a9e2aa613860b26d4f5a7abb88576aa42b3432ad210357f7ed4c118e581f49cd3b4d9dd1edb4295f4def49d6dcf2faaaaac87a1a0a42210372cd46831f3b6afd4c044d160b7667e8ebf659d6cb51a825a3104df6ee0638c62103ae72827d25030818c4947a800187b1fbcc33ae751e248ae60094cc989fb880f62103b3a7aa25702000c5c1faa300600e8e2bd89cde2be7fb1ec898a39c50d9de90d12103b53899c390573471ba30e5054f78376c5f797fda26dde7a760789f02908cbad22103e05bf6002b62651378b1954820539c36ca405cbb778c225395dd9ebff67802992103ecd8af1e93c57a1b8c7f917bd9980af798adeb0205e9687865673353eb041e8d5daeffffffff02c2f2aa09000000001976a914fb53759d716de10362f5b57d28151a6481fd30e188ac132c42980000000017a914596cff92a275960df9cb2ab9df0ff69faa2b1d8a8700000000";

/// The other, `415c14…`: in bucket 5 of a table of 16, where the first is
/// in bucket 15.
const FIRST_OF_16_BUCKETS: &str = "0100000001961576fed5c343511f011b32e11681b90ab0df1e33fde103913698476fd8ddca01000000fdc80100000000000000004dbd0157210231a395e332dde8688800a0025cccc5771ea1aa874a633b8ab6e5c89d300c7c3621026b472f7d59d201ff1f540f111b6eb329e071c30a9d23e3d2bcd128fe73dc254c21027319afb15481dbeb3c426bcc37f9a30e7f51ceff586936d85548d9395bcc2344210294c817150f78607566e961b3c71df53a22022a80acbb982f83c0c8baac040adc2103250c11be0561b1d7ae168b1f59e39cbc1fd1ba3cf4d2140c1a365b2723a2bf9321033ada6ef3b1d93a1978b595c7a9e2aa613860b26d4f5a7abb88576aa42b3432ad210357f7ed4c118e581f49cd3b4d9dd1edb4295f4def49d6dcf2faaaaac87a1a0a42210372cd46831f3b6afd4c044d160b7667e8ebf659d6cb51a825a3104df6ee0638c62103ae72827d25030818c4947a800187b1fbcc33ae751e248ae60094cc989fb880f62103b3a7aa25702000c5c1faa300600e8e2bd89cde2be7fb1ec898a39c50d9de90d12103b53899c390573471ba30e5054f78376c5f797fda26dde7a760789f02908cbad22103e05bf6002b62651378b1954820539c36ca405cbb778c225395dd9ebff67802992103ecd8af1e93c57a1b8c7f917bd9980af798adeb0205e9687865673353eb041e8d5daeffffffff027e0c3012000000001976a9149fc530162154c490cb5ca484b8ad883d0bf18a1788ac48e38d410100000017a914596cff92a275960df9cb2ab9df0ff69faa2b1d8a8700000000";

/// The block the two were made at.
const MADE_AT: u64 = 3_340_302;

fn raw(hex: &str) -> Vec<u8> {
    alloy_primitives::hex::decode(hex).expect("hex")
}

/// Waiting for confirmations, each under its creation transaction's hash:
/// the two peg-outs above, and nine made a block before the Bridge's, none
/// of them confirmed. Eleven, as `BridgeStorageProvider` reads them.
fn eleven_waiting(collections: &mut Collections) {
    let number = collections.bridge.number();
    let mut entries: Vec<PegoutWaitingForConfirmations> =
        [FIRST_OF_32_BUCKETS, FIRST_OF_16_BUCKETS]
            .into_iter()
            .zip(0x40u8..)
            .map(|(hex, tag)| PegoutWaitingForConfirmations {
                btc_tx_raw: raw(hex),
                rsk_block_height: MADE_AT,
                rsk_tx_hash: Some([tag; 32]),
            })
            .collect();
    for tag in 0x60..0x69u8 {
        entries.push(PegoutWaitingForConfirmations {
            btc_tx_raw: bitcoin::consensus::serialize(&btc_transaction(
                vec![input(tag, vec![])],
                &[(BTC, user(tag))],
            )),
            rsk_block_height: number - 1,
            rsk_tx_hash: Some([tag; 32]),
        });
    }
    peg::store_pegout_confirmation_set(&mut collections.bridge.ctx, &entries, true);
}

/// The one peg-out waiting for signatures.
fn waiting_for_signatures(collections: &mut Collections) -> Vec<u8> {
    let kept = peg::deserialize_rsk_txs_waiting_for_signatures(
        &named(&mut collections.bridge.ctx, "rskTxsWaitingFS").unwrap_or_default(),
    );
    assert_eq!(kept.len(), 1, "one peg-out waits for signatures");
    kept.into_values().next().expect("one")
}

/// rskj reads eleven, and adds the batch the call builds to that same set:
/// the twelfth leaves its table of 16 three quarters full and not more, so
/// it is walked at 16, where a set of twelve read from storage would be
/// walked at 32.
#[test]
fn a_pegout_built_in_the_call_leaves_the_set_walked_at_the_size_it_was_read() {
    let mut collections = Collections::mainnet_at(MAINNET_HOP400);
    collections.active(1, 10);
    collections.requests(3);
    eleven_waiting(&mut collections);
    collections.update();
    assert_eq!(
        collections.waiting().len(),
        11,
        "one taken, the batch added"
    );
    assert_eq!(
        waiting_for_signatures(&mut collections),
        raw(FIRST_OF_16_BUCKETS)
    );
}

/// The migration and the batch the call builds both go into the set it
/// read, and the thirteenth doubles its table to 32.
#[test]
fn a_migration_and_a_pegout_built_in_the_call_double_the_set_walked() {
    let mut collections = Collections::migrating(MAINNET_HOP400, STANDARD_MULTISIG, 10, false);
    collections.requests(3);
    eleven_waiting(&mut collections);
    collections.update();
    assert_eq!(collections.waiting().len(), 12, "one taken, two added");
    assert_eq!(
        waiting_for_signatures(&mut collections),
        raw(FIRST_OF_32_BUCKETS)
    );
}
