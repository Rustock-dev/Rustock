//! rskj's `PocSighashTest`: what the legacy signature hash of a peg-out's
//! input depends on, the hash a member signs and the pegout index keys by
//! (`BtcTransaction.hashForSignature(index, redeem, ALL, false)`). Each input
//! has its own; it covers no scriptSig, so an input signed and unsigned has
//! the same; it changes with the transaction; and over a peg-out testnet's
//! federators signed and broadcast, it is what rskj computes. The federation
//! is a P2SH ERP one of keys from seeds, with three emergency keys after 720
//! blocks. rskj runs three of the tests on mainnet's and on testnet's
//! parameters, which name addresses alone: the hashes are the same on both,
//! and the ports run once.

use std::collections::BTreeSet;

use bitcoin::{
    absolute::LockTime, transaction::Version, Amount, Network, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxOut, Witness,
};
use k256::ecdsa::{signature::hazmat::PrehashSigner, Signature, SigningKey};

use crate::bridge::peg::p2sh_erp_redeem_script;
use crate::bridge::release_tx::{legacy_sighash_all, placeholder_scriptsig, push_data};
use crate::bridge::script_spend::script_correctly_spends_tx;

use super::support::{p2pkh, p2sh};

/// The emergency keys' activation delay, in blocks.
const DELAY: i64 = 720;

/// The emergency keys, by seed.
const EMERGENCY: [&str; 3] = ["erp-fed-01", "erp-fed-02", "erp-fed-03"];

/// A federation of seven, by seed.
const SEVEN: [&str; 7] = ["fed1", "fed2", "fed3", "fed4", "fed5", "fed6", "fed7"];

/// Testnet's federators of the peg-out below, by seed.
const TESTNET_FEDERATORS: [&str; 3] = ["federator1", "federator2", "federator6"];

/// Where testnet's federation of [`TESTNET_FEDERATORS`] is.
const TESTNET_ADDRESS: &str = "2NEfaGq4tGe6bJUxLEzGFoVyZrSrZtXRzJ7";

/// The transactions that paid that federation what the peg-out spends, with
/// the output of each it spends.
const FUNDING: [(&str, u32); 4] = [
    ("0200000000010134ed7734da14ecde305347153f70be45021e8e29137205a9899630f56c68d3890100000000fdffffff02891300000000000017a914eaf58ece160a383630667cfc1ccff519ab07c472873c3e020000000000160014f885b26136ad4d61247132271795cc29ae9cec0302473044022063986423838cdd2abc51b150f11a1c399dce53f68f1bd02e4ec793db2fa3485c022007d8909a83b01c57dcee7e55f3989af949756c6b5068948b888622d2e6673ed70121028f117bfbc90d934b73d0d55afe54ee33a288cdde572bd3d006cede67f4c797a2e7262500", 0),
    ("020000000001013a43f0d972f5045b443c9c071bd2254f91d5b14d0b5ab5cdeab014c0ff2cc17f0200000000fdffffff027d6408000000000017a914eaf58ece160a383630667cfc1ccff519ab07c472870555dc0500000000220020617a81e635a62aa61d20fd4be369d285da2750c871016282c05a29181552d52b0400473044022048386d4e0270b68a17d3ea10d8c569c2150cb46416c82b3289f2e0ec1fe0272a02204f077bdc8f71d824a4a8a447708cb974e21c015021b94f8226a3da955024371e0147304402204fda66a5cc3503acd325671216f5dbfe4d329499321af508c4959e89072dc47802204531cb9a52c80ede1cab8ca40324ef18ddfb589a0b76f741764334f6f1e9570f014752210203307a637032952b21371f966838e03a768220f8bb663cbc2aa6d41dc71f6bbf2103fdadd0c267fc7a4890c37c6730ca124d53ad7001ee6d9ad00ed50b52a46d784552aee7262500", 0),
    ("020000000001016a040fa1ec01bac865d4802083a435fef32052091d6d7bd02cc784d00d75e8400100000000fdffffff02916408000000000017a914eaf58ece160a383630667cfc1ccff519ab07c47287beefd305000000002200201ca17d3f37320ac4469faa1119824d98bbec3b1a586174901834d2b842715e7c0400473044022044033baafc0c15c8e2c3128d01f29a7c7c5c6ec790ead8c5be2610efb0e9d581022014b605cb59a5254a189a9fcd35ec94e6f4faae18f7011a41ac7663df33fbfdc50147304402206fc734a5db737ecb37056491881da15c2f8f2ca2bb9e47db7396029b9a0ad5e102206cae6f8766679c666cfa256e886e2816882623167e92fd9e272fd9712aa49d9a0147522102b9bb9ea5c5c0a56421bc7720e81e7f3e5fb9dfcbca946f01d868940efaa5da18210309eb318fe76a6b139d70e4037dd87733ced9e3ed6f851301a88938c7033d473b52ae05272500", 0),
    ("0200000001a223ad7e9d4f5067a8fdddf9e4137af49d5a6dad7ea952533f701061fefd010f020000006a473044022003d999e14be1e2ea15cb16df81e14b30f26c684bcbf6fcbd07975d8648e5f04402207e99a414a0ebf1c3617944610de5ea002439a1ac56e667a60c095184c2419559012102c6bf1e099ec95510a8da3d1c67026cb65a019bad57dc2255dd56a426c629327bfdffffff0300000000000000001b6a1952534b540162db6c4b118d7259c23692b162829e6bd5e4d5b0891300000000000017a914eaf58ece160a383630667cfc1ccff519ab07c4728791200000000000001976a9145952b24450e80668e069b8152a3a38ea7f6ad44c88ace7262500", 1),
];

/// The peg-out the federators signed and testnet confirmed.
const BROADCAST: &str = "02000000044928841a8bcded3af365da1b57b98b96e4593893862599c504f883b0c956751600000000fd700100483045022100d05a33bd8d0032ed9ccd5f3ba1fb085166c9155f03d95a79e65ef9cf60b9e4b602201287eedce988adf75d8daadc978eca43f47bc33ec999652008e85a3d3dd47f8001483045022100eb5f917967f772328dfd82bd27a096cac453ca336a51d1782ca6684304dd34d5022055bd25e56ed2702974fb58224481e393fca12c4ff34771dab5f456c8fa76568401004cda64522103462ab7041341dadd996dc12ef0c118ca8ccb546498cbf304f7ffe0f1b12f9a9e210362634ab57dae9cb373a5d536e66a8c4f67468bbcfb063809bab643072d78a1242103c5946b3fbae03a654237da863c9ed534e0878657175b132b8ca630f245df04db53ae6702d002b2755221029cecea902067992d52c38b28bf0bb2345bda9b21eca76b16a17c477a64e433012103284178e5fbcc63c54c3b38e3ef88adf2da6c526313650041b0ef955763634ebd2103b9fc46657cf72a1afa007ecf431de1cd27ff5cc8829fa625b66ca47b967e6b2453ae68ffffffff6a040fa1ec01bac865d4802083a435fef32052091d6d7bd02cc784d00d75e84000000000fd6e010047304402206a00bb7a876ecbacd13c06a3e8ca3c55d022e9f35ad6211c65bc4ab3e2a12f1b022051a1d0861a5e812c6c2ffa1be0467fe953ceff6ecb9fc3eb68843441d5c3d7a501473044022044744cceb33a776928c61c8d01cf3acf58c7159ccd5a69914cb78be2a59d5b2d022020ff71156583ba563e9a4d6a8f0bd463d53a6612e831082e0ac9c1c2d1a6d79101004cda64522103462ab7041341dadd996dc12ef0c118ca8ccb546498cbf304f7ffe0f1b12f9a9e210362634ab57dae9cb373a5d536e66a8c4f67468bbcfb063809bab643072d78a1242103c5946b3fbae03a654237da863c9ed534e0878657175b132b8ca630f245df04db53ae6702d002b2755221029cecea902067992d52c38b28bf0bb2345bda9b21eca76b16a17c477a64e433012103284178e5fbcc63c54c3b38e3ef88adf2da6c526313650041b0ef955763634ebd2103b9fc46657cf72a1afa007ecf431de1cd27ff5cc8829fa625b66ca47b967e6b2453ae68ffffffff114c09191b5b1fe685640c23a2ab95bfec521d49ddde4bb96032471762b441e400000000fd6f0100483045022100979a3c231a8ca8c1aa081dffe6cbe0d7ad5987ce4329757cfd8e0c3bfbc5ba8c02202357deaf74162fefc1a05ddf4c2bb2b7054fa105ceddf6ed4e061dab88ae056f0147304402204ebb79cbc3fd9260e28b1f9c4564f3352ce3adba6dc390279808d42961152c9a022034c215c5e9fc88b1622d0636950b5dd1de7b1322e9857736f9a3a63e9bb2cdad01004cda64522103462ab7041341dadd996dc12ef0c118ca8ccb546498cbf304f7ffe0f1b12f9a9e210362634ab57dae9cb373a5d536e66a8c4f67468bbcfb063809bab643072d78a1242103c5946b3fbae03a654237da863c9ed534e0878657175b132b8ca630f245df04db53ae6702d002b2755221029cecea902067992d52c38b28bf0bb2345bda9b21eca76b16a17c477a64e433012103284178e5fbcc63c54c3b38e3ef88adf2da6c526313650041b0ef955763634ebd2103b9fc46657cf72a1afa007ecf431de1cd27ff5cc8829fa625b66ca47b967e6b2453ae68ffffffffefd39f04aa64aec104c68bbecef06dc8ffa916cec5c536b8e03a19e40b2f9af101000000fd700100483045022100a668d635ca8528be0b0affc22c64636f2128b1a1a0891a5f3d2c2579eb9ab4ea0220450088cb20042ce52ebd32c61f2d3b5085c6cd725c36ffae1a751878091a81b601483045022100e25e9ee15ac340063a27258ca905c273cd6fa907271c3d5ddcf1daea2969ddc6022059f190175e98636708bbc4c9cb9809216ee09a0f0d2794e9bf87220567e43c7401004cda64522103462ab7041341dadd996dc12ef0c118ca8ccb546498cbf304f7ffe0f1b12f9a9e210362634ab57dae9cb373a5d536e66a8c4f67468bbcfb063809bab643072d78a1242103c5946b3fbae03a654237da863c9ed534e0878657175b132b8ca630f245df04db53ae6702d002b2755221029cecea902067992d52c38b28bf0bb2345bda9b21eca76b16a17c477a64e433012103284178e5fbcc63c54c3b38e3ef88adf2da6c526313650041b0ef955763634ebd2103b9fc46657cf72a1afa007ecf431de1cd27ff5cc8829fa625b66ca47b967e6b2453ae68ffffffff0124e710000000000017a9140e39a099dabb03863ae5484943896a7c9eb9f9dd8700000000";

/// What the peg-out pays, and to whom.
const PAID: u64 = 1_107_748;
const PAID_TO: &str = "2MtYSUzFWEQV62r92bsGX8ewE5Mgpv4xn9M";

/// rskj's `FedSigner.listOf`: the keys of the private keys `keccak256(seed)`,
/// in the order of their public keys.
fn signers(seeds: &[&str]) -> Vec<SigningKey> {
    use sha3::Digest;
    let mut signers: Vec<SigningKey> = seeds
        .iter()
        .map(|seed| {
            SigningKey::from_slice(&sha3::Keccak256::digest(seed.as_bytes())).expect("a key")
        })
        .collect();
    signers.sort_by_key(public);
    signers
}

/// A key's compressed public key.
fn public(key: &SigningKey) -> [u8; 33] {
    key.verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// rskj's `getSignaturesPermutations`: the majorities of `n` signers it signs
/// with, each a signer and a run of signers after it.
fn majorities(n: usize) -> Vec<Vec<usize>> {
    let run = n / 2;
    let mut all = Vec::new();
    for first in 0..n {
        for start in first + 1..n {
            if start + run > n {
                break;
            }
            all.push(std::iter::once(first).chain(start..start + run).collect());
        }
    }
    all
}

/// A P2SH ERP federation of members' keys, with [`EMERGENCY`]'s after
/// [`DELAY`] blocks.
struct Federation {
    members: Vec<SigningKey>,
    emergency: Vec<SigningKey>,
    redeem: Vec<u8>,
}

impl Federation {
    fn of(members: &[&str]) -> Self {
        let members = signers(members);
        let emergency = signers(&EMERGENCY);
        let keys = |signers: &[SigningKey]| signers.iter().map(public).collect::<Vec<_>>();
        let redeem = p2sh_erp_redeem_script(&keys(&members), &keys(&emergency), DELAY);
        Self {
            members,
            emergency,
            redeem,
        }
    }

    /// Its P2SH output script.
    fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::new_p2sh(&ScriptBuf::from_bytes(self.redeem.clone()).script_hash())
    }

    /// Its address on testnet.
    fn testnet_address(&self) -> String {
        bitcoin::Address::p2sh(
            &ScriptBuf::from_bytes(self.redeem.clone()),
            Network::Testnet,
        )
        .expect("a redeem script of at most 520 bytes")
        .to_string()
    }

    /// The sighash of input `index` of `tx`.
    fn sighash(&self, tx: &Transaction, index: usize) -> [u8; 32] {
        legacy_sighash_all(tx, index, &self.redeem)
    }

    /// rskj's `spendFromFed`: a version 2 transaction spending `outpoints`,
    /// all the federation's, paying `amount` satoshis to `to`. Each input is
    /// signed in turn by each of the [`majorities`] of the members, or with
    /// `emergency` of the emergency keys, each input's sequence then the
    /// delay; each time it spends the federation's output as bitcoinj runs
    /// it, and the last majority's signatures stay. The sighash of each input
    /// is taken before it is signed.
    fn spend(
        &self,
        emergency: bool,
        outpoints: &[OutPoint],
        amount: u64,
        to: &ScriptBuf,
    ) -> (Transaction, Vec<[u8; 32]>) {
        let sequence = if emergency {
            Sequence(DELAY as u32)
        } else {
            Sequence::MAX
        };
        let mut tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: outpoints
                .iter()
                .map(|&previous_output| TxIn {
                    previous_output,
                    script_sig: ScriptBuf::new(),
                    sequence,
                    witness: Witness::new(),
                })
                .collect(),
            output: vec![TxOut {
                value: Amount::from_sat(amount),
                script_pubkey: to.clone(),
            }],
        };
        let signers = if emergency {
            &self.emergency
        } else {
            &self.members
        };
        let output = self.script_pubkey();
        let mut sighashes = Vec::new();
        for index in 0..tx.input.len() {
            let sighash = self.sighash(&tx, index);
            sighashes.push(sighash);
            for majority in majorities(signers.len()) {
                let mut script_sig = vec![0x00];
                for &signer in &majority {
                    let signature: Signature =
                        signers[signer].sign_prehash(&sighash).expect("signed");
                    let mut encoded = signature.to_der().as_bytes().to_vec();
                    encoded.push(0x01);
                    push_data(&mut script_sig, &encoded);
                }
                // The branch of the redeem script: OP_0 the members', OP_1 the
                // emergency keys'.
                script_sig.push(if emergency { 0x51 } else { 0x00 });
                push_data(&mut script_sig, &self.redeem);
                tx.input[index].script_sig = ScriptBuf::from_bytes(script_sig);
                assert!(
                    script_correctly_spends_tx(&tx, index, output.as_bytes()),
                    "input {index} signed by {majority:?}"
                );
            }
        }
        (tx, sighashes)
    }
}

/// A transaction of no inputs paying each of `outputs`, as rskj's tests make
/// a peg-in, and the output of it the federation holds.
fn paying(outputs: &[(u64, ScriptBuf)], held: u32) -> (Transaction, u32) {
    let tx = Transaction {
        version: Version::ONE,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: outputs
            .iter()
            .map(|(value, script_pubkey)| TxOut {
                value: Amount::from_sat(*value),
                script_pubkey: script_pubkey.clone(),
            })
            .collect(),
    };
    (tx, held)
}

/// What the outputs `held` hold, and where they are.
fn outpoints_of(held: &[(Transaction, u32)]) -> (u64, Vec<OutPoint>) {
    let total = held
        .iter()
        .map(|(tx, index)| tx.output[*index as usize].value.to_sat())
        .sum();
    let outpoints = held
        .iter()
        .map(|(tx, index)| OutPoint::new(tx.compute_txid(), *index))
        .collect();
    (total, outpoints)
}

/// Someone outside the federation: rskj's random P2PKH address.
fn someone() -> ScriptBuf {
    ScriptBuf::from_bytes(p2pkh(&[0x55; 20]))
}

/// The P2SH outputs of the [`FUNDING`] transactions.
fn funding() -> Vec<OutPoint> {
    FUNDING
        .iter()
        .map(|(raw, index)| {
            let bytes = alloy_primitives::hex::decode(raw).expect("hex");
            let tx: Transaction = bitcoin::consensus::deserialize(&bytes).expect("a transaction");
            OutPoint::new(tx.compute_txid(), *index)
        })
        .collect()
}

/// [`PAID_TO`]'s output script.
fn paid_to() -> ScriptBuf {
    PAID_TO
        .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
        .expect("an address")
        .require_network(Network::Testnet)
        .expect("testnet's")
        .script_pubkey()
}

/// The [`BROADCAST`] peg-out.
fn broadcast() -> Transaction {
    let bytes = alloy_primitives::hex::decode(BROADCAST).expect("hex");
    bitcoin::consensus::deserialize(&bytes).expect("a transaction")
}

/// rskj's `test_each_input_sighash_is_unique`: nine outputs the federation
/// holds, seven of peg-ins and two beside an output to another, a P2PKH and
/// a multisig one, are the inputs of a peg-out, each with a sighash of its
/// own, which the signed peg-out keeps.
#[test]
fn each_input_of_a_pegout_has_a_sighash_of_its_own() {
    let fed = Federation::of(&SEVEN);
    let mut held: Vec<(Transaction, u32)> = (1..=7)
        .map(|i| paying(&[(100_000 * i, fed.script_pubkey())], 0))
        .collect();
    held.push(paying(
        &[(500_001, someone()), (500_002, fed.script_pubkey())],
        1,
    ));
    let multisig = ScriptBuf::from_bytes(p2sh(&[0x66; 20]));
    held.push(paying(
        &[(700_001, multisig), (700_002, fed.script_pubkey())],
        1,
    ));
    let (total, outpoints) = outpoints_of(&held);
    let (pegout, sighashes) = fed.spend(false, &outpoints, total - 15_000, &someone());
    let distinct: BTreeSet<[u8; 32]> = sighashes.iter().copied().collect();
    assert_eq!(distinct.len(), held.len());
    for index in 0..pegout.input.len() {
        assert!(
            distinct.contains(&fed.sighash(&pegout, index)),
            "input {index}"
        );
    }
}

/// rskj's `test_sighash_is_different_when_tx_is_altered`: a peg-out of one
/// output the federation holds, then one of it and another: no input of the
/// second has a sighash of the first.
#[test]
fn a_pegout_of_one_more_output_has_none_of_the_sighashes() {
    let fed = Federation::of(&SEVEN);
    let mut held = vec![paying(&[(100_000, fed.script_pubkey())], 0)];
    let (total, outpoints) = outpoints_of(&held);
    let (_, first) = fed.spend(false, &outpoints, total - 15_000, &someone());
    assert_eq!(first.len(), 1);

    held.push(paying(&[(100_000, fed.script_pubkey())], 0));
    let (total, outpoints) = outpoints_of(&held);
    let (_, second) = fed.spend(false, &outpoints, total - 15_000, &someone());
    let distinct: BTreeSet<[u8; 32]> = second.iter().copied().collect();
    assert_eq!(distinct.len(), held.len());
    assert!(first.iter().all(|sighash| !distinct.contains(sighash)));
}

/// rskj's `test_sighash_is_equal_for_signed_input_and_unsigned_input`: the
/// inputs of a signed peg-out given back the scriptSig the Bridge builds
/// them with, before any signature, have the sighashes they were signed over.
#[test]
fn an_input_has_its_sighash_signed_or_not() {
    let fed = Federation::of(&["fed1", "fed2", "fed3"]);
    let held: Vec<(Transaction, u32)> = (1..=7)
        .map(|i| paying(&[(100_000 * i, fed.script_pubkey())], 0))
        .collect();
    let (total, outpoints) = outpoints_of(&held);
    let (mut pegout, sighashes) = fed.spend(false, &outpoints, total - 15_000, &someone());
    assert_eq!(sighashes.iter().collect::<BTreeSet<_>>().len(), held.len());
    let threshold = fed.members.len() / 2 + 1;
    for input in &mut pegout.input {
        input.script_sig = ScriptBuf::from_bytes(placeholder_scriptsig(&fed.redeem, threshold));
    }
    for (index, sighash) in sighashes.iter().enumerate() {
        assert_eq!(&fed.sighash(&pegout, index), sighash, "input {index}");
    }
}

/// rskj's `test_each_input_sighash_is_unique_using_real_tx_testnet`: the
/// peg-out testnet's federation of three built from four outputs it held,
/// signed again by its members, has the sighashes of the one they signed and
/// broadcast, input by input.
#[test]
fn a_pegout_signed_again_has_the_sighashes_of_the_one_testnet_confirmed() {
    let fed = Federation::of(&TESTNET_FEDERATORS);
    assert_eq!(fed.testnet_address(), TESTNET_ADDRESS);
    let (pegout, sighashes) = fed.spend(false, &funding(), PAID, &paid_to());
    assert_eq!(
        sighashes.iter().collect::<BTreeSet<_>>().len(),
        FUNDING.len()
    );
    let broadcast = broadcast();
    for (index, sighash) in sighashes.iter().enumerate() {
        assert_eq!(&fed.sighash(&pegout, index), sighash, "input {index}");
        assert_eq!(&fed.sighash(&broadcast, index), sighash, "input {index}");
    }
}

/// rskj's `test_each_input_sighash_is_unique_for_a_signed_erp_tx_testnet`:
/// the same peg-out signed by the emergency keys, its inputs' sequences the
/// delay, has a sighash of its own at each input, none the broadcast one's.
#[test]
fn the_emergency_keys_sign_other_sighashes_than_the_members() {
    let fed = Federation::of(&TESTNET_FEDERATORS);
    assert_eq!(fed.testnet_address(), TESTNET_ADDRESS);
    let (pegout, sighashes) = fed.spend(true, &funding(), PAID, &paid_to());
    assert_eq!(
        sighashes.iter().collect::<BTreeSet<_>>().len(),
        FUNDING.len()
    );
    let broadcast = broadcast();
    for index in 0..pegout.input.len() {
        assert_ne!(
            fed.sighash(&pegout, index),
            fed.sighash(&broadcast, index),
            "input {index}"
        );
    }
}

/// rskj's `test_redeemScript_can_be_obtained_from_input`: testnet's
/// federation of three is at its address. rskj's test then reads the redeem
/// script each input of a transaction pushes last, of a transaction it gives
/// no inputs; here an input as the Bridge builds it pushes the federation's.
#[test]
fn an_inputs_redeem_script_is_the_federations() {
    let fed = Federation::of(&TESTNET_FEDERATORS);
    assert_eq!(fed.testnet_address(), TESTNET_ADDRESS);
    let threshold = fed.members.len() / 2 + 1;
    let script_sig = ScriptBuf::from_bytes(placeholder_scriptsig(&fed.redeem, threshold));
    let pushed_last = match script_sig.instructions().last() {
        Some(Ok(bitcoin::script::Instruction::PushBytes(bytes))) => bytes.as_bytes().to_vec(),
        other => panic!("no redeem script pushed last: {other:?}"),
    };
    assert_eq!(pushed_last, fed.redeem);
}
