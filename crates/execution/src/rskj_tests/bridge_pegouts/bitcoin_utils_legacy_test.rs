//! rskj's `BitcoinUtilsLegacyTest` (`co/rsk/peg/bitcoin/BitcoinUtilsLegacyTest.java`):
//! a multisig transaction's hash with its signatures taken out as rskj takes
//! them out before RSKIP305
//! (`BitcoinUtilsLegacy.getMultiSigTransactionHashWithoutSignaturesBeforeRSKIP305`),
//! and the transaction with them taken out as it does from RSKIP305
//! (`BitcoinUtils.getMultiSigTransactionWithoutSignatures`), over which a
//! peg-out's sighash is taken for the RSKIP379 index. rustock's
//! `peg::multisig_tx_hash_without_signatures` and
//! `peg::without_multisig_signatures` are the two; rskj's
//! `IllegalArgumentException` is their `Ok(None)`.
//!
//! rskj's `BtcTransaction.equals` compares two transactions' hashes, which
//! leave the witnesses out; the ports compare the whole transactions. Its two
//! tests of an input whose key reads as a script are held, through the
//! Bridge, by `bridge_support_register_btc_transaction_test.rs`.

use bitcoin::hashes::Hash;

use crate::bridge::{peg, release_tx, rskj_sender_compat};

use super::support::*;

/// rskj `BitcoinTestUtils.getBtcEcKeyFromSeed`: the private key
/// `keccak256(seed)`.
fn key_of(seed: &str) -> k256::ecdsa::SigningKey {
    use sha3::Digest;
    k256::ecdsa::SigningKey::from_slice(&sha3::Keccak256::digest(seed.as_bytes())).expect("a key")
}

/// A key's compressed public key.
fn public_of(key: &k256::ecdsa::SigningKey) -> [u8; 33] {
    key.verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// rskj `getBtcEcKeysFromSeeds` of `member01` up to `count`, sorted: the
/// keys in the order of their public keys.
fn members(count: usize) -> Vec<k256::ecdsa::SigningKey> {
    let mut keys: Vec<_> = (1..=count)
        .map(|n| key_of(&format!("member{n:02}")))
        .collect();
    keys.sort_by_key(public_of);
    keys
}

/// The redeem script of rskj's `P2shErpFederationBuilder` (nine members) and
/// `P2shP2wshErpFederationBuilder` (twenty): the members' keys, with
/// mainnet's emergency keys and delay. Both formats spend by it.
fn erp_redeem(count: usize) -> Vec<u8> {
    let constants = BridgeConstants::mainnet();
    let members: Vec<[u8; 33]> = members(count).iter().map(public_of).collect();
    let emergency: Vec<[u8; 33]> = constants
        .erp_fed_pubkeys
        .iter()
        .map(|key| {
            alloy_primitives::hex::decode(key)
                .expect("hex")
                .try_into()
                .expect("33 bytes")
        })
        .collect();
    peg::p2sh_erp_redeem_script(&members, &emergency, constants.erp_fed_activation_delay)
}

/// bitcoinj `Script.getNumberOfSignaturesRequiredToSpend` of `redeem`.
fn required(redeem: &[u8]) -> usize {
    let required = rskj_sender_compat::signatures_required_to_spend(redeem).expect("a multisig");
    usize::try_from(required).expect("a count")
}

/// An input spending output 0 of rskj's `BitcoinTestUtils.createHash(n)`,
/// `n` in the hash's first byte, which bitcoinj writes last.
fn spending(n: u8, script_sig: Vec<u8>) -> bitcoin::TxIn {
    let mut hash = [0u8; 32];
    hash[31] = n;
    bitcoin::TxIn {
        previous_output: bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array(hash),
            vout: 0,
        },
        script_sig: bitcoin::ScriptBuf::from_bytes(script_sig),
        sequence: bitcoin::Sequence::MAX,
        witness: bitcoin::Witness::new(),
    }
}

/// rskj `ScriptBuilder.createInputScript(null, key)` for the key of the seed
/// `abc`: no signature, then the key, whose bytes don't read as a script.
fn spend_by_the_key_of_abc() -> Vec<u8> {
    [vec![0x00], push(&public_of(&key_of("abc")))].concat()
}

/// bitcoinj `getHash()`: a transaction's txid, which leaves its witnesses
/// out, in the order the two functions answer in.
fn hash_of(tx: &bitcoin::Transaction) -> [u8; 32] {
    let mut hash = tx.compute_txid().to_byte_array();
    hash.reverse();
    hash
}

/// `signer`'s signature of `sighash`, as bitcoinj writes it into an input.
fn signature(signer: &k256::ecdsa::SigningKey, sighash: &[u8; 32]) -> Vec<u8> {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let signed: k256::ecdsa::Signature = signer.sign_prehash(sighash).expect("signed");
    release_tx::signature_to_insert(signed.to_der().as_bytes(), sighash, &public_of(signer))
        .expect("it verifies")
}

/// rskj `BitcoinTestUtils.signLegacyTransactionInputFromP2shMultiSig` and
/// `signWitnessTransactionInputFromP2shMultiSig`: input `index` of `tx`,
/// spending `value` satoshis by `redeem`, signed by as many of `signers`, in
/// their order, as the script asks for, each signature where bitcoinj puts
/// it.
fn sign(
    tx: &mut bitcoin::Transaction,
    index: usize,
    value: u64,
    redeem: &[u8],
    signers: &[k256::ecdsa::SigningKey],
) {
    let witness = !tx.input[index].witness.is_empty();
    let sighash = if witness {
        release_tx::segwit_sighash_all(tx, index, redeem, value)
    } else {
        release_tx::legacy_sighash_all(tx, index, redeem)
    };
    for signer in signers.iter().take(required(redeem)) {
        let public = public_of(signer);
        let signature = signature(signer, &sighash);
        if witness {
            let pushes: Vec<Vec<u8>> = tx.input[index].witness.iter().map(<[u8]>::to_vec).collect();
            let at = release_tx::witness_sig_insertion_index(&pushes, &sighash, &public, redeem)
                .expect("no point at infinity");
            let signed = release_tx::witness_update_with_signature(&pushes, &signature, at)
                .expect("a slot left");
            tx.input[index].witness = bitcoin::Witness::from_slice(&signed);
        } else {
            let script_sig = tx.input[index].script_sig.as_bytes().to_vec();
            let at = release_tx::sig_insertion_index(&script_sig, &sighash, &public, redeem)
                .expect("no point at infinity");
            let signed = release_tx::update_script_with_signature(&script_sig, &signature, at)
                .expect("a slot left");
            tx.input[index].script_sig = bitcoin::ScriptBuf::from_bytes(signed);
        }
    }
}

/// `getMultiSigTransactionHashWithoutSignatures_whenTransactionDoesNotHaveInputs_shouldReturnSameTxHash`:
/// a transaction of no inputs has nothing taken out, and keeps its hash.
#[test]
fn a_transaction_of_no_inputs_keeps_its_hash() {
    let tx = btc_transaction(Vec::new(), &[]);
    assert_eq!(
        peg::multisig_tx_hash_without_signatures(&tx, false),
        Ok(Some(hash_of(&tx)))
    );
    assert_eq!(
        peg::without_multisig_signatures(&tx, false),
        Ok(Some(tx.clone()))
    );
}

/// `getMultiSigTransactionHashWithoutSignatures_whenTransactionIsSegwit_shouldReturnExpectedTxHash`:
/// a P2SH-P2WSH ERP federation's input, as the Bridge leaves it
/// (`addSpendingFederationBaseScript`), signed by five of the eleven its
/// twenty members are asked for. A segwit transaction's hash is its own,
/// which its signatures don't change, and with them taken out it is the
/// transaction it was before them.
#[test]
fn a_segwit_transaction_keeps_its_hash_and_is_unsigned_as_it_was() {
    let redeem = erp_redeem(20);
    let mut tx = btc_transaction(vec![spending(1, Vec::new())], &[]);
    let (script_sig, witness) = release_tx::segwit_base_input(&redeem, required(&redeem));
    tx.input[0].script_sig = script_sig;
    tx.input[0].witness = witness;
    let before = tx.clone();

    sign(&mut tx, 0, BTC, &redeem, &members(5));
    assert_ne!(tx, before, "signed");

    let hash = peg::multisig_tx_hash_without_signatures(&tx, false);
    assert_eq!(hash, Ok(Some(hash_of(&tx))));
    assert_eq!(hash, Ok(Some(hash_of(&before))));
    let unsigned = peg::without_multisig_signatures(&tx, false).expect("no throw");
    assert_eq!(unsigned.as_ref(), Some(&before));
    assert_eq!(hash, Ok(unsigned.as_ref().map(hash_of)));
}

/// `getMultiSigTransactionHashWithoutSignatures_whenTxIsNotSegwitAndTransactionInputsDoNotHaveP2shMultiSigInputScript_shouldThrowIAE`:
/// an input whose last push doesn't read as a script has no redeem script,
/// and both throw `IllegalArgumentException`.
#[test]
fn an_input_of_no_redeem_script_fails_both() {
    let script_sig = spend_by_the_key_of_abc();
    assert_eq!(
        rskj_sender_compat::redeem_script_of_input(&script_sig, None),
        Ok(None)
    );
    let tx = btc_transaction(vec![spending(1, script_sig)], &[]);
    assert_eq!(
        peg::multisig_tx_hash_without_signatures(&tx, false),
        Ok(None)
    );
    assert_eq!(peg::without_multisig_signatures(&tx, false), Ok(None));
}

/// `getMultiSigTransactionHashWithoutSignatures_whenTxIsNotSegwitButNotAllTransactionInputsHaveP2shMultiSigInputScript_shouldThrowIAE`:
/// a P2SH ERP federation's input with no signatures, its redeem script read,
/// and one whose last push doesn't read as a script: both throw
/// `IllegalArgumentException`.
#[test]
fn one_input_of_no_redeem_script_fails_both() {
    let redeem = erp_redeem(9);
    let multisig = release_tx::placeholder_scriptsig(&redeem, required(&redeem));
    assert_eq!(
        rskj_sender_compat::redeem_script_of_input(&multisig, None),
        Ok(Some(redeem.clone()))
    );
    let by_a_key = spend_by_the_key_of_abc();
    assert_eq!(
        rskj_sender_compat::redeem_script_of_input(&by_a_key, None),
        Ok(None)
    );
    let tx = btc_transaction(vec![spending(2, multisig), spending(1, by_a_key)], &[]);
    assert_eq!(
        peg::multisig_tx_hash_without_signatures(&tx, false),
        Ok(None)
    );
    assert_eq!(peg::without_multisig_signatures(&tx, false), Ok(None));
}

/// `getMultiSigTransactionHashWithoutSignatures_whenTransactionIsLegacyAndInputsHaveP2shMultiSigInputScript_shouldReturnExpectedTxHash`:
/// two inputs of a P2SH ERP federation of nine, paying a bitcoin to the
/// address of the seed `destinationAddress`, each signed by the five its
/// script asks for. With them taken out it is the transaction it was before
/// them, and its hash is that one's.
#[test]
fn a_legacy_transaction_is_unsigned_as_it_was() {
    let redeem = erp_redeem(9);
    let unsigned = release_tx::placeholder_scriptsig(&redeem, required(&redeem));
    let destination = p2pkh(&hash160(&public_of(&key_of("destinationAddress"))));
    let mut tx = btc_transaction(
        vec![spending(1, unsigned.clone()), spending(2, unsigned)],
        &[(BTC, destination)],
    );
    let before = tx.clone();

    for index in 0..tx.input.len() {
        sign(&mut tx, index, 0, &redeem, &members(5));
    }
    assert_ne!(hash_of(&tx), hash_of(&before), "signed");

    let unsigned = peg::without_multisig_signatures(&tx, false).expect("no throw");
    assert_eq!(unsigned.as_ref(), Some(&before));
    assert_eq!(
        peg::multisig_tx_hash_without_signatures(&tx, false),
        Ok(Some(hash_of(&before)))
    );
    assert_eq!(unsigned.as_ref().map(hash_of), Some(hash_of(&before)));
}
