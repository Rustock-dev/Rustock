//! rskj `co.rsk.peg.PegUtilsLegacyTest` and `PegUtilsLegacyGetTransactionTypeTest`:
//! how the legacy rules take a Bitcoin transaction, as a peg-in
//! (`isValidPegInTx`), a migration (`isMigrationTx`, or from RSKIP199 a spend of
//! the old federation) or a peg-out (`isPegOutTx`), by era.
//!
//! rskj applies them where `registerBtcTransaction` reads no pegout index:
//! before RSKIP379, and below the index's height after it. These tests
//! register each transaction at Bitcoin height 349, below rskj's regtest
//! index (250 and a grace of 100), and read what the Bridge did: whether the
//! call succeeded, whether it marked the transaction processed, the events
//! it logged, and the UTXOs the active federation holds. A transaction no
//! rule takes is left alone, `(true, false, 0, 0)`; a peg-out or a migration
//! is marked processed with no event, its outputs to the federation kept.

use bitcoin::hashes::Hash as _;
use revm::context_interface::{ContextTr, JournalTr};

use super::support::{self, NEW_FEDERATION, OLD_FEDERATION};
use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{peg, serialization, storage};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::BridgeTxContext;

const BLOCK: u64 = 1_000;
const BTC: u64 = 100_000_000;

/// rskj's regtest from genesis, or with `upgrade` and every one after it moved
/// past the block these tests run at.
fn regtest(before: Option<RskNetworkUpgrade>) -> RskHardforkConfig {
    let all = RskHardforkConfig::rskj_regtest();
    before.map_or(all.clone(), |upgrade| {
        support::upgrades_from_at(all, upgrade, 1_000_000)
    })
}

/// What the Bridge holds for a federation: its keys, its format and its
/// redeem script.
struct Federation {
    keys: Vec<[u8; 33]>,
    format: u64,
    redeem: Vec<u8>,
}

/// rskj's regtest delay, 500, as a Java number little-endian.
const REGTEST_DELAY: &[u8] = &[0xf4, 0x01];

fn standard(first: u8) -> Federation {
    let keys = support::keys(first, 3);
    let redeem = support::standard_redeem(&keys);
    Federation {
        keys,
        format: 1000,
        redeem,
    }
}

fn p2sh_erp(first: u8) -> Federation {
    let keys = support::keys(first, 3);
    let emergency = support::erp_keys_of(&BridgeConstants::rskj_regtest());
    let redeem = support::p2sh_erp_redeem(&keys, &emergency, REGTEST_DELAY);
    Federation {
        keys,
        format: 3000,
        redeem,
    }
}

fn address_of(redeem: &[u8]) -> bitcoin::ScriptBuf {
    support::p2sh(&support::hash160(redeem))
}

/// `redeem` behind a Flyover derivation hash: `<hash> OP_DROP <redeem>`.
fn flyover(redeem: &[u8]) -> Vec<u8> {
    [&[0x20][..], &[0x01; 32], &[0x75], redeem].concat()
}

/// A non-standard ERP redeem script of `keys`, with the emergency keys of
/// the private keys 0xfa01 and 0xfa02 and a delay of 500, as rskj's tests
/// build it.
fn non_standard_erp(keys: &[[u8; 33]]) -> Vec<u8> {
    let emergency = [0xfa01u16, 0xfa02].map(|k| support::public_key(&k.to_be_bytes()));
    support::non_standard_erp_redeem(keys, &emergency, &[0x01, 0xf4])
}

/// `data` pushed as bitcoinj pushes it.
fn push(data: &[u8]) -> Vec<u8> {
    let mut out = match data.len() {
        n @ 0..=75 => vec![n as u8],
        n @ 76..=255 => vec![0x4c, n as u8],
        n => [&[0x4d][..], &(n as u16).to_le_bytes()].concat(),
    };
    out.extend_from_slice(data);
    out
}

/// A multisig spend of `redeem`: OP_0, two signatures' worth of bytes, and
/// the redeem script pushed last.
fn spending(redeem: &[u8]) -> Vec<u8> {
    [
        &[0x00][..],
        &push(&[0x30; 71]),
        &push(&[0x31; 71]),
        &push(redeem),
    ]
    .concat()
}

/// `tx` with its first input a spend of `redeem` signed by the private keys
/// `signers`, as rskj's `FederationTestUtils.addSignatures` signs one: OP_0,
/// their signatures for SIGHASH_ALL in the order their public keys stand in
/// `redeem` (first a signer's whose key isn't in it), an OP_0 more to take an
/// ERP script's default branch, and `redeem`. The signature hash is Bitcoin's
/// legacy one, which bitcoinj's is for a transaction with no witness.
fn signed(
    tx: bitcoin::Transaction,
    redeem: &[u8],
    signers: &[&[u8]],
    erp: bool,
) -> bitcoin::Transaction {
    let hash = bitcoin::sighash::SighashCache::new(&tx)
        .legacy_signature_hash(0, bitcoin::Script::from_bytes(redeem), 1)
        .expect("the first input")
        .to_byte_array();
    signed_over(tx, hash, redeem, signers, erp)
}

/// bitcoinj's signature hash of the first input for SIGHASH_ALL: Bitcoin's
/// legacy one, but over the copy serialized with its witnesses where the
/// transaction has any (`BtcTransaction.hashForSignature`).
fn bitcoinj_sighash(tx: &bitcoin::Transaction, redeem: &[u8]) -> [u8; 32] {
    let mut copy = tx.clone();
    for input in &mut copy.input {
        input.script_sig = bitcoin::ScriptBuf::new();
    }
    copy.input[0].script_sig = bitcoin::ScriptBuf::from_bytes(redeem.to_vec());
    let mut preimage = bitcoin::consensus::serialize(&copy);
    preimage.extend_from_slice(&1u32.to_le_bytes());
    bitcoin::hashes::sha256d::Hash::hash(&preimage).to_byte_array()
}

/// [`signed`], over the signature hash `hash`.
fn signed_over(
    mut tx: bitcoin::Transaction,
    hash: [u8; 32],
    redeem: &[u8],
    signers: &[&[u8]],
    erp: bool,
) -> bitcoin::Transaction {
    use k256::ecdsa::signature::hazmat::PrehashSigner as _;
    let mut signers = signers.to_vec();
    signers.sort_by_key(|private| {
        let key = support::public_key(private);
        redeem.windows(33).position(|window| window == key)
    });
    let mut script_sig = vec![0x00];
    for private in signers {
        let mut scalar = [0u8; 32];
        scalar[32 - private.len()..].copy_from_slice(private);
        let key = k256::ecdsa::SigningKey::from_slice(&scalar).expect("a private key in range");
        let signature: k256::ecdsa::Signature = key.sign_prehash(&hash).expect("a signature");
        script_sig.extend(push(&[signature.to_der().as_bytes(), &[0x01]].concat()));
    }
    if erp {
        script_sig.push(0x00);
    }
    script_sig.extend(push(redeem));
    tx.input[0].script_sig = bitcoin::ScriptBuf::from_bytes(script_sig);
    tx
}

/// A transaction whose inputs have the scriptSigs `inputs`, paying `outputs`.
fn tx(inputs: &[Vec<u8>], outputs: &[(u64, bitcoin::ScriptBuf)]) -> bitcoin::Transaction {
    let mut tx = support::tx_paying(outputs);
    tx.input = inputs
        .iter()
        .enumerate()
        .map(|(i, script_sig)| bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([0x5b; 32]),
                vout: i as u32,
            },
            script_sig: bitcoin::ScriptBuf::from_bytes(script_sig.clone()),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        })
        .collect();
    tx
}

/// A P2PKH spend's scriptSig: a signature's bytes and a public key.
fn p2pkh_spend() -> Vec<u8> {
    [push(&[0x30; 71]), push(&support::public_key(&[0x77]))].concat()
}

/// What `registerBtcTransaction` does with `tx` at height 349, at block
/// 1,000 under `cfg`, with `active` the active federation, `retiring` the
/// retiring one, and `retired` the last retired federation's P2SH hash.
fn registered(
    cfg: &RskHardforkConfig,
    active: &Federation,
    retiring: Option<&Federation>,
    retired: Option<[u8; 20]>,
    tx: &bitcoin::Transaction,
) -> (bool, bool, usize, usize) {
    let raw = bitcoin::consensus::serialize(tx);
    let mut ctx = support::ctx_at(BLOCK);
    support::btc_chain_holding(&mut ctx, &raw, cfg);
    support::fund_bridge(&mut ctx);
    support::store_federation(&mut ctx, NEW_FEDERATION, &active.keys, 1, active.format);
    if let Some(retiring) = retiring {
        support::store_federation(&mut ctx, OLD_FEDERATION, &retiring.keys, 0, retiring.format);
    }
    if let Some(retired) = retired {
        let script = support::p2sh(&retired).into_bytes();
        storage::bridge_store_bytes_named(
            &mut ctx,
            storage::LAST_RETIRED_FEDERATION_P2SH_SCRIPT_KEY,
            &serialization::rlp_encode_list(&[serialization::rlp_encode_element(&script)]),
        );
    }
    let tree = support::one_leaf_tree(&raw);
    let args = encode::args(&[
        Arg::Bytes(&raw),
        Arg::Word(encode::int(349)),
        Arg::Bytes(&tree),
    ]);
    let config = BridgeConstants::rskj_regtest();
    let ok = peg::register_btc_transaction(
        &mut ctx,
        &args,
        0,
        &config,
        cfg,
        &BridgeTxContext::default(),
    )
    .is_ok();
    let logs = ctx.journal_mut().take_logs().len();
    let txid = crate::bridge::tx::legacy_btc_txid(tx);
    let processed = crate::bridge::tx::is_btc_tx_processed(&mut ctx, &txid);
    let utxos = storage::load_federation_utxos(&mut ctx, cfg, BLOCK).len();
    (ok, processed, logs, utxos)
}

const LEFT_ALONE: (bool, bool, usize, usize) = (true, false, 0, 0);

/// A peg-out or a migration: marked processed with no event, keeping
/// `utxos` outputs to the active federation.
fn moved(utxos: usize) -> (bool, bool, usize, usize) {
    (true, true, 0, utxos)
}

/// Whether the transaction was taken and marked processed, as a peg-in is,
/// accepted or refunded.
fn taken(outcome: (bool, bool, usize, usize)) -> bool {
    outcome.0 && outcome.1
}

/// rskj's `isValidPegInTx_*_iris` tests: from RSKIP219 a peg-in must send the
/// federation 500,000 satoshis, before it 1,000,000 (rskj's regtest); one
/// that sends less is no peg-in, and is left alone. These come from a P2PKH
/// sender, whose peg-in is taken.
#[test]
fn a_peg_in_must_send_the_minimum_of_its_era() {
    let active = standard(1);
    let paying = |value| tx(&[p2pkh_spend()], &[(value, address_of(&active.redeem))]);
    let after = regtest(None);
    let before = regtest(Some(RskNetworkUpgrade::Iris300));
    assert_eq!(
        registered(&after, &active, None, None, &paying(250_000)),
        LEFT_ALONE
    );
    for value in [750_000, 50 * BTC] {
        assert!(
            taken(registered(&after, &active, None, None, &paying(value))),
            "{value} from RSKIP219"
        );
    }
    assert_eq!(
        registered(&before, &active, None, None, &paying(750_000)),
        LEFT_ALONE
    );
    assert!(taken(registered(
        &before,
        &active,
        None,
        None,
        &paying(1_000_000)
    )));
}

/// rskj's `isValidPegInTx_spending_from_federation_is_pegout_after_iris`: a
/// spend of the active federation that pays it back is no peg-in but a
/// peg-out.
#[test]
fn a_spend_of_the_federation_is_a_pegout() {
    let active = standard(1);
    let tx = tx(
        &[spending(&active.redeem)],
        &[(BTC, address_of(&active.redeem))],
    );
    assert_eq!(
        registered(&regtest(None), &active, None, None, &tx),
        moved(1)
    );
}

/// Before RSKIP201 `isValidPegInTx` refuses a spend of the federation only as
/// one of its script, its signatures checked: its members' spend that pays it
/// back is no peg-in but a peg-out, and the same redeem script under
/// signatures that don't verify is a peg-in.
#[test]
fn before_rskip201_a_spend_of_the_federation_is_a_pegout_where_its_members_sign() {
    let active = standard(1);
    let paying_it_back = || {
        tx(
            &[spending(&active.redeem)],
            &[(BTC, address_of(&active.redeem))],
        )
    };
    let cfg = regtest(Some(RskNetworkUpgrade::Iris300));
    let by_members = signed(paying_it_back(), &active.redeem, &[&[1], &[3]], false);
    assert_eq!(registered(&cfg, &active, None, None, &by_members), moved(1));
    let unsigned = registered(&cfg, &active, None, None, &paying_it_back());
    assert!(taken(unsigned) && unsigned.2 > 0);
}

/// From RSKIP201 `isValidPegInTx` compares an input's standard redeem script
/// with the federation's byte for byte: a multisig of the federation's keys
/// in another order is another script, its spend no pegout, and the
/// transaction, paying the federation, a peg-in.
#[test]
fn from_rskip201_a_multisig_of_the_federations_keys_in_another_order_is_not_its() {
    let active = standard(1);
    let mut keys = active.keys.clone();
    keys.sort();
    keys.reverse();
    let mut reversed = vec![0x52];
    for key in &keys {
        reversed.extend(push(key));
    }
    reversed.extend([0x53, 0xae]);
    let tx = tx(&[spending(&reversed)], &[(BTC, address_of(&active.redeem))]);
    let outcome = registered(&regtest(None), &active, None, None, &tx);
    assert!(taken(outcome) && outcome.2 > 0);
}

/// `isValidPegInTx` reads each input's redeem script, whatever the upgrade,
/// and bitcoinj parses a scriptSig to find it there: one it can't parse
/// throws, nothing catches it, and the call fails, though the transaction
/// pays the federation nothing.
#[test]
fn a_script_sig_bitcoinj_cannot_parse_fails_the_call() {
    let active = standard(1);
    let tx = tx(&[vec![0x4c]], &[(BTC, support::p2pkh(&[0x99; 20]))]);
    for cfg in [regtest(None), regtest(Some(RskNetworkUpgrade::Iris300))] {
        assert_eq!(
            registered(&cfg, &active, None, None, &tx),
            (false, false, 0, 0)
        );
    }
}

/// A federation's redeem script behind a Flyover prefix, within a
/// non-standard ERP script, and both.
fn wrapped(federation: &Federation) -> [(&'static str, Vec<u8>); 3] {
    [
        ("flyover", flyover(&federation.redeem)),
        ("non-standard erp", non_standard_erp(&federation.keys)),
        (
            "flyover non-standard erp",
            flyover(&non_standard_erp(&federation.keys)),
        ),
    ]
}

/// rskj's `isValidPegInTx_hasChangeUtxoFrom*_afterRskip201_notPegin`: from
/// RSKIP201 a spend of the active federation's redeem script wrapped is no
/// peg-in but a peg-out, and one of the last retired federation's, paying
/// the active one, is a migration.
#[test]
fn from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in() {
    let active = standard(1);
    let retired = standard(11);
    let to_active = [(BTC, address_of(&active.redeem))];
    for (of, federation, retired_hash) in [
        ("active", &active, None),
        ("retired", &retired, Some(support::hash160(&retired.redeem))),
    ] {
        for (shape, redeem) in wrapped(federation) {
            let tx = tx(&[spending(&redeem)], &to_active);
            assert_eq!(
                registered(&regtest(None), &active, None, retired_hash, &tx),
                moved(1),
                "{shape} of the {of} federation"
            );
        }
    }
}

/// rskj's `isValidPegInTx_hasChangeUtxoFrom{Flyover,FlyoverErp,Erp}Federation
/// _beforeRskip201_isPegin`, with the change input as rskj writes it, the
/// redeem script itself its scriptSig: before RSKIP201 only an input that
/// spends a federation's P2SH script makes a transaction no peg-in
/// (PegUtilsLegacy.java:186-194), and this one spends none, so it is taken
/// for a peg-in, not a peg-out. Its sender is read next, and the reader of a
/// multisig sender takes the scriptSig's last chunk, here an opcode, for the
/// redeem script, whose null bytes bitcoinj cannot parse
/// (P2shMultisigBtcLockSender.java:52-54): the call fails, where a peg-out
/// would have been marked processed.
#[test]
fn before_rskip201_the_change_of_a_flyover_or_erp_federation_is_a_peg_in() {
    let active = standard(1);
    for (shape, redeem) in wrapped(&active) {
        let tx = tx(&[redeem], &[(BTC, address_of(&active.redeem))]);
        assert_eq!(
            registered(
                &regtest(Some(RskNetworkUpgrade::Iris300)),
                &active,
                None,
                None,
                &tx
            ),
            (false, false, 0, 0),
            "{shape}"
        );
    }
}

/// rskj's `isPegOutTx_from{Flyover,Erp,FlyoverErp}Federation`, from RSKIP201:
/// a spend of the active federation's redeem script wrapped, paying
/// elsewhere, is a peg-out (PegUtilsLegacy.java isPegOutTx, the input's
/// standard redeem script against the federation's).
#[test]
fn from_rskip201_a_spend_of_a_wrapped_federation_redeem_script_is_a_pegout() {
    let active = standard(1);
    for (shape, redeem) in wrapped(&active) {
        let tx = tx(&[spending(&redeem)], &[(BTC, support::p2pkh(&[0x99; 20]))]);
        assert_eq!(
            registered(&regtest(None), &active, None, None, &tx),
            moved(0),
            "{shape}"
        );
    }
}

/// The same tests before RSKIP201: rskj's `isPegOutTx` compares the P2SH
/// script of the input's redeem script as it is, wrapped, to the
/// federation's, and `isValidPegInTx` finds no spend of the federation's P2SH
/// script either, while the transaction sends the federation nothing. It is
/// neither, and left alone.
#[test]
fn before_rskip201_a_spend_of_a_wrapped_federation_redeem_script_is_no_pegout() {
    let active = standard(1);
    for (shape, redeem) in wrapped(&active) {
        let tx = tx(&[spending(&redeem)], &[(BTC, support::p2pkh(&[0x99; 20]))]);
        assert_eq!(
            registered(
                &regtest(Some(RskNetworkUpgrade::Iris300)),
                &active,
                None,
                None,
                &tx
            ),
            LEFT_ALONE,
            "{shape}"
        );
    }
}

/// rskj's `isValidPegInTx_has_*_RSKIP293` tests: before RSKIP293 what the
/// federation is sent counts in total; from it each UTXO must be worth the
/// minimum, 500,000 satoshis.
#[test]
fn from_rskip293_each_utxo_of_a_peg_in_must_be_worth_the_minimum() {
    let active = standard(1);
    let to = |values: &[u64]| {
        let outputs: Vec<_> = values
            .iter()
            .map(|&v| (v, address_of(&active.redeem)))
            .collect();
        tx(&[p2pkh_spend()], &outputs)
    };
    let min = 500_000;
    let before = regtest(Some(RskNetworkUpgrade::Hop400));
    let after = regtest(None);
    assert!(taken(registered(
        &before,
        &active,
        None,
        None,
        &to(&[min / 4; 4])
    )));
    assert_eq!(
        registered(
            &before,
            &active,
            None,
            None,
            &to(&[min / 4, min / 4, min / 4, min / 5])
        ),
        LEFT_ALONE
    );
    assert_eq!(
        registered(
            &after,
            &active,
            None,
            None,
            &to(&[min / 2, min / 2, min / 2, min, min + BTC])
        ),
        LEFT_ALONE
    );
    assert!(taken(registered(&after, &active, None, None, &to(&[min]))));
}

/// rskj's `isValidPegInTx_*P2shErpScript_sends_funds_to_*_after_RSKIP353`: a
/// spend of a P2SH ERP script of the active federation's members, or of its
/// Flyover, is no peg-in, whether it pays the federation or elsewhere: it is
/// a peg-out.
#[test]
fn a_spend_of_a_p2sh_erp_script_of_the_federations_members_is_a_pegout() {
    let active = standard(1);
    let p2sh_erp = support::p2sh_erp_redeem(&active.keys, &support::keys(31, 3), &[0x00, 0x01]);
    for redeem in [p2sh_erp.clone(), flyover(&p2sh_erp)] {
        for (to, utxos) in [
            (address_of(&active.redeem), 1),
            (support::p2pkh(&[0x99; 20]), 0),
        ] {
            let tx = tx(&[spending(&redeem)], &[(BTC, to)]);
            assert_eq!(
                registered(&regtest(None), &active, None, None, &tx),
                moved(utxos)
            );
        }
    }
}

/// rskj's `testTxIsProcessableInLegacyVersion`: before RSKIP143 a legacy
/// peg-in is registered only from a P2PKH sender, from it also from a P2SH
/// multisig one (BridgeUtils.txIsProcessableInLegacyVersion); before it the
/// peg-in is left alone (BridgeSupport.java:659-666), from it, its sender
/// being one rskj does not lock for, refunded.
#[test]
fn before_rskip143_a_peg_in_is_taken_only_from_a_p2pkh_sender() {
    let active = standard(1);
    let to_federation = [(BTC, address_of(&active.redeem))];
    let multisig = tx(
        &[spending(&support::standard_redeem(&support::keys(21, 3)))],
        &to_federation,
    );
    let before = regtest(Some(RskNetworkUpgrade::Papyrus200));
    let after = regtest(Some(RskNetworkUpgrade::Iris300));
    assert_eq!(
        registered(&before, &active, None, None, &multisig),
        LEFT_ALONE,
        "before RSKIP143"
    );
    assert!(
        taken(registered(&after, &active, None, None, &multisig)),
        "from RSKIP143"
    );
}

/// rskj's `isMigrationTx_sending_funds_from_*`: a spend of the retiring or the
/// last retired federation, P2SH ERP or standard, that pays the active one,
/// P2SH ERP or standard, is a migration. The last retired P2SH ERP
/// federation is known by the P2SH script of its members' multisig; by its
/// whole ERP script it is not, and its members' spend, which `isValidPegInTx`
/// refuses as a peg-in for spending that script, is left alone.
#[test]
fn a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration() {
    let cfg = regtest(None);
    for (from, to) in [
        (p2sh_erp(11), p2sh_erp(1)),
        (standard(11), p2sh_erp(1)),
        (standard(11), standard(1)),
    ] {
        let tx = tx(&[spending(&from.redeem)], &[(BTC, address_of(&to.redeem))]);
        assert_eq!(
            registered(&cfg, &to, Some(&from), None, &tx),
            moved(1),
            "retiring"
        );
        let members = support::hash160(&support::standard_redeem(&from.keys));
        assert_eq!(
            registered(&cfg, &to, None, Some(members), &tx),
            moved(1),
            "retired"
        );
    }
    let (from, to) = (p2sh_erp(11), p2sh_erp(1));
    let tx = signed(
        tx(&[spending(&from.redeem)], &[(BTC, address_of(&to.redeem))]),
        &from.redeem,
        &[&[11], &[12]],
        true,
    );
    assert_eq!(
        registered(&cfg, &to, None, Some(support::hash160(&from.redeem)), &tx),
        LEFT_ALONE,
        "retired, known by its whole script"
    );
}

/// rskj's regtest old federation's private keys
/// (`PegUtilsLegacyGetTransactionTypeTest`'s
/// `REGTEST_OLD_FEDERATION_PRIVATE_KEYS`).
const REGTEST_OLD_FEDERATION_PRIVATE_KEYS: [&str; 3] = [
    "47129ffed2c0273c75d21bb8ba020073bb9a1638df0e04853407461fdd9e8b83",
    "9f72d27ba603cfab5a0201974a6783ca2476ec3d6b4e2625282c682e0e5f1c35",
    "e1b17fcd0ef1942465eee61b20561b16750191143d365e71de08b33dd84a9788",
];

/// The old federation's private keys and its redeem script, two of the three.
fn old_federation() -> (Vec<Vec<u8>>, Vec<u8>) {
    let privates: Vec<Vec<u8>> = REGTEST_OLD_FEDERATION_PRIVATE_KEYS
        .iter()
        .map(|private| hex::decode(private).expect("a private key in hex"))
        .collect();
    let keys: Vec<[u8; 33]> = privates
        .iter()
        .map(|private| support::public_key(private))
        .collect();
    (privates, support::standard_redeem(&keys))
}

/// rskj's `PegUtilsLegacyGetTransactionTypeTest.test_sentFromOldFed`: a
/// transaction two of the old federation's three members sign, paying the
/// active federation, is a peg-in before RSKIP199 and from it a migration,
/// whose output the active federation keeps. From RSKIP186 the old federation
/// is the last retired one too. The old federation is rskj regtest's
/// `oldFederationAddress`.
#[test]
fn from_rskip199_a_spend_of_the_old_federation_is_a_migration() {
    let (privates, old) = old_federation();
    assert_eq!(
        support::hash160(&old),
        BridgeConstants::rskj_regtest().old_federation_address_hash160
    );
    let active = standard(1);
    let tx = signed(
        tx(&[spending(&old)], &[(BTC, address_of(&active.redeem))]),
        &old,
        &[&privates[0], &privates[1]],
        false,
    );
    let before = registered(
        &regtest(Some(RskNetworkUpgrade::Iris300)),
        &active,
        None,
        None,
        &tx,
    );
    assert!(taken(before) && before.2 > 0, "a peg-in before RSKIP199");
    assert_eq!(
        registered(
            &regtest(None),
            &active,
            None,
            Some(support::hash160(&old)),
            &tx
        ),
        moved(1)
    );
}

/// From RSKIP199 an input is the old federation's where it spends the old
/// federation's script as bitcoinj runs it, its signatures checked
/// (`PegUtilsLegacy.scriptCorrectlySpendsTx`). With no retired federation to
/// know it by otherwise, the old federation's redeem script under signatures
/// that don't all verify, one by a key not among its members' or none at all,
/// makes no migration: the transaction, paying the active federation, is a
/// peg-in.
#[test]
fn a_spend_of_the_old_federation_counts_only_where_its_signatures_verify() {
    let (privates, old) = old_federation();
    let active = standard(1);
    let paying = || tx(&[spending(&old)], &[(BTC, address_of(&active.redeem))]);
    let cfg = regtest(None);
    let members = signed(paying(), &old, &[&privates[0], &privates[2]], false);
    assert_eq!(registered(&cfg, &active, None, None, &members), moved(1));
    let outsider = signed(paying(), &old, &[&privates[0], &[0x77]], false);
    for (signed_by, tx) in [("an outsider", outsider), ("no one", paying())] {
        let outcome = registered(&cfg, &active, None, None, &tx);
        assert!(taken(outcome) && outcome.2 > 0, "signed by {signed_by}");
    }
}

/// bitcoinj reads a spend from the scriptSig, and hashes a transaction for a
/// legacy signature with its witnesses where it has any, as Bitcoin doesn't.
/// In a transaction with a witness on another input, the old federation's
/// spend signed as Bitcoin signs is no spend of it to rskj, and the
/// transaction, paying the active federation, is a peg-in; signed over
/// bitcoinj's hash, it is a migration. An input that carries the old
/// federation's redeem script in its witness, its scriptSig empty, spends
/// nothing bitcoinj runs either.
#[test]
fn a_spend_of_the_old_federation_is_read_from_its_script_sig_as_bitcoinj_hashes_it() {
    let (privates, old) = old_federation();
    let active = standard(1);
    let cfg = regtest(None);
    let to_active = [(BTC, address_of(&active.redeem))];
    let mut paying = tx(&[spending(&old), p2pkh_spend()], &to_active);
    paying.input[1].witness =
        bitcoin::Witness::from_slice(&[vec![0x30; 71], support::public_key(&[0x77]).to_vec()]);
    let signers: [&[u8]; 2] = [&privates[0], &privates[1]];
    let as_bitcoin_signs = signed(paying.clone(), &old, &signers, false);
    let outcome = registered(&cfg, &active, None, None, &as_bitcoin_signs);
    assert!(taken(outcome) && outcome.2 > 0, "signed as Bitcoin signs");
    let hash = bitcoinj_sighash(&paying, &old);
    let over_bitcoinjs_hash = signed_over(paying, hash, &old, &signers, false);
    assert_eq!(
        registered(&cfg, &active, None, None, &over_bitcoinjs_hash),
        moved(1)
    );
    let mut in_a_witness = tx(&[p2pkh_spend(), vec![]], &to_active);
    in_a_witness.input[1].witness =
        bitcoin::Witness::from_slice(&[vec![], vec![0x30; 71], vec![0x31; 71], old.clone()]);
    let outcome = registered(&cfg, &active, None, None, &in_a_witness);
    assert!(
        taken(outcome) && outcome.2 > 0,
        "the redeem script in a witness"
    );
}
