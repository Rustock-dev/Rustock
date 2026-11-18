//! rskj's `PegUtilsLegacyGetTransactionTypeTest`: how the legacy rules
//! classify a transaction, here a spend of the old federation, which
//! RSKIP186 (the last retired federation) and RSKIP199 (the old federation's
//! address) make a migration.

use super::support::*;

/// rskj regtest's `oldFederationAddress`.
const REGTEST_OLD_FEDERATION_ADDRESS: &str = "2N7ZgQyhFKm17RbaLqygYbS7KLrQfapyZzu";

/// rskj's `REGTEST_OLD_FEDERATION_PRIVATE_KEYS`.
const OLD_FEDERATION_PRIVATE_KEYS: [&str; 3] = [
    "47129ffed2c0273c75d21bb8ba020073bb9a1638df0e04853407461fdd9e8b83",
    "9f72d27ba603cfab5a0201974a6783ca2476ec3d6b4e2625282c682e0e5f1c35",
    "e1b17fcd0ef1942465eee61b20561b16750191143d365e71de08b33dd84a9788",
];

fn private(hex: &str) -> Vec<u8> {
    alloy_primitives::hex::decode(hex).expect("a private key in hex")
}

/// Their public keys.
fn old_federation() -> Vec<[u8; 33]> {
    OLD_FEDERATION_PRIVATE_KEYS
        .iter()
        .map(|key| public_key(&private(key)))
        .collect()
}

/// `tx` with its first input a spend of `redeem`, signed as rskj's
/// `FederationTestUtils.addSignatures` signs it: OP_0, a SIGHASH_ALL
/// signature by each of the first two of the old federation's keys, in the
/// order their public keys stand in `redeem`, and `redeem` pushed last. The
/// transaction has no witness, so Bitcoin's legacy signature hash is
/// bitcoinj's.
fn signed_by_the_old_federation(
    mut tx: bitcoin::Transaction,
    redeem: &[u8],
) -> bitcoin::Transaction {
    use bitcoin::hashes::Hash as _;
    use k256::ecdsa::signature::hazmat::PrehashSigner as _;
    let hash = bitcoin::sighash::SighashCache::new(&tx)
        .legacy_signature_hash(0, bitcoin::Script::from_bytes(redeem), 1)
        .expect("the first input")
        .to_byte_array();
    let mut signers: Vec<Vec<u8>> = OLD_FEDERATION_PRIVATE_KEYS[..2]
        .iter()
        .map(|key| private(key))
        .collect();
    signers.sort_by_key(|signer| {
        let key = public_key(signer);
        redeem.windows(33).position(|window| window == key)
    });
    let mut script_sig = vec![0x00];
    for signer in signers {
        let key = k256::ecdsa::SigningKey::from_slice(&signer).expect("a private key in range");
        let signature: k256::ecdsa::Signature = key.sign_prehash(&hash).expect("a signature");
        script_sig.extend(push(&[signature.to_der().as_bytes(), &[0x01]].concat()));
    }
    script_sig.extend(push(redeem));
    tx.input[0].script_sig = bitcoin::ScriptBuf::from_bytes(script_sig);
    tx
}

/// rskj's `test_sentFromOldFed`: a spend of regtest's old federation, at
/// `2N7ZgQyhFKm17RbaLqygYbS7KLrQfapyZzu`, signed by two of its three members,
/// to the active federation. Under papyrus200, before RSKIP186 and RSKIP199,
/// it is a peg-in, from a multisig sender, and is refunded; under iris300 it
/// is a migration, its output registered to the active federation, whether
/// or not the old federation is also stored as the last retired one.
#[test]
fn from_rskip199_a_spend_of_the_old_federation_is_a_migration() {
    use std::str::FromStr;
    let redeem = standard_redeem(&old_federation());
    let address = bitcoin::Address::from_str(REGTEST_OLD_FEDERATION_ADDRESS)
        .expect("an address")
        .assume_checked();
    assert_eq!(
        address.script_pubkey().into_bytes(),
        p2sh(&hash160(&redeem)),
        "the old federation's keys are those of its address"
    );
    let tx = signed_by_the_old_federation(
        btc_transaction(
            vec![input(1, Vec::new())],
            &[(100_000_000, address_of(&active()))],
        ),
        &redeem,
    );
    let at = |last: RskNetworkUpgrade, last_retired: bool| {
        let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
        let mut bridge = Registration::new(cfg, BridgeConstants::rskj_regtest(), 1_000);
        bridge.federation(NEW_FEDERATION, &active());
        if last_retired {
            put_named(
                &mut bridge.ctx,
                "lastRetiredFedP2SHScript",
                &rlp_encode_list(&[rlp_encode_element(&p2sh(&hash160(&redeem)))]),
            );
        }
        bridge.register(&tx, 1_000)
    };

    let outcome = at(RskNetworkUpgrade::Papyrus200, false);
    assert!(outcome.returned);
    assert_eq!(outcome.events, vec![Event::ReleaseRequested(100_000_000)]);
    assert_eq!(outcome.processed_at, Some(1_000));
    assert!(outcome.active_utxos.is_empty());
    assert_eq!(outcome.refunds, 1);

    for last_retired in [true, false] {
        let outcome = at(RskNetworkUpgrade::Iris300, last_retired);
        assert!(outcome.returned);
        assert!(outcome.events.is_empty(), "last retired {last_retired}");
        assert_eq!(outcome.processed_at, Some(1_000));
        assert_eq!(
            outcome.active_utxos,
            vec![100_000_000],
            "last retired {last_retired}"
        );
        assert_eq!(outcome.refunds, 0);
    }
}
