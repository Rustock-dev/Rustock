//! rskj's `BridgeSupportFlyoverTest`: Flyover peg-ins, which the liquidity
//! bridge contract (the LBC) registers with `registerFastBridgeBtcTransaction`.
//!
//! rskj's tests call `BridgeSupport.registerFlyoverBtcTransaction` with mocks
//! for what it reads: the federations its storage provider holds, a Bitcoin
//! block store whose block at the transaction's height holds it with enough
//! blocks above, the locking cap, the Bridge's balance. Here the same state is
//! written where the Bridge reads it, and the call is the contract's: the
//! Bridge's method, from the LBC, a call deep. rskj's tests switch single
//! RSKIPs on; this port takes the upgrade that brings them: iris300 for
//! RSKIP176 (Flyover), hop400 for RSKIP293 (the retiring federation's
//! Flyover address).
//!
//! Some of the set-up differs. The Bitcoin addresses the call names are of
//! the test's network, as the Bridge's method reads them, where rskj's tests
//! hand `BridgeSupport` mainnet ones. On mainnet the block is the first at
//! which the new federation is active by mainnet's own activation age, where
//! rskj's tests lend mainnet regtest's. Where rskj's tests make the active
//! federation the network's genesis keys as a standard multisig
//! (`getGenesisFederationLegacy`), those keys are stored so, as the new
//! federation. And where rskj's tests leave the locking cap (RSKIP134) off or
//! mock it away, it is on here and the Bridge holds all the bitcoin there is,
//! so that none is locked.

use super::support::*;
use crate::bridge::btc_store::{put_stored_block, store_chain_head, StoredBlock};
use crate::bridge::federation::{serialize_federation_multikey, StoredMember};
use crate::bridge::serialization::{rlp_encode_element, rlp_encode_list, rlp_encode_u64};
use crate::bridge::storage::{
    bridge_load_raw, bridge_store_bytes_named, bridge_store_u256, compound_key,
    FAST_BRIDGE_FEDERATION_INFO_KEY, LOCKING_CAP_KEY, NEW_FEDERATION_FORMAT_VERSION_KEY,
    NEW_FEDERATION_KEY, OLD_FEDERATION_FORMAT_VERSION_KEY, OLD_FEDERATION_KEY,
    RELEASES_OUTPOINTS_VALUES_KEY,
};
use crate::precompiles::BRIDGE_ADDR;
use alloy_primitives::{Address, B256, U256};
use bitcoin::hashes::Hash;
use RskNetworkUpgrade::{Hop400, Iris300, Lovell700, Vetiver900};

const COIN: u64 = 100_000_000;
const CENT: u64 = 1_000_000;

const SIGNATURE: &str =
    "registerFastBridgeBtcTransaction(bytes,uint256,bytes,bytes32,bytes,address,bytes,bool)";

/// The LBC. rskj's tests call from the zero address.
const LBC: Address = Address::ZERO;

/// rskj's `FlyoverTxResponseCodes`.
const REFUNDED_USER: i64 = -100;
const REFUNDED_LP: i64 = -200;
const NOT_CONTRACT: i64 = -300;
const INVALID_SENDER: i64 = -301;
const ALREADY_PROCESSED: i64 = -302;
const VALIDATIONS: i64 = -303;
const VALUE_ZERO: i64 = -304;
const UTXO_BELOW_MINIMUM: i64 = -305;
const GENERIC_ERROR: i64 = -900;

#[derive(Clone, Copy, Debug)]
enum Net {
    Regtest,
    Mainnet,
}

impl Net {
    fn constants(self) -> BridgeConstants {
        match self {
            Net::Regtest => BridgeConstants::rskj_regtest(),
            Net::Mainnet => BridgeConstants::mainnet(),
        }
    }

    fn chain_id(self) -> u64 {
        match self {
            Net::Regtest => RSK_REGTEST_CHAIN_ID,
            Net::Mainnet => RSK_MAINNET_CHAIN_ID,
        }
    }

    /// The version byte of a P2PKH address.
    fn p2pkh(self) -> u8 {
        match self {
            Net::Regtest => 111,
            Net::Mainnet => 0,
        }
    }

    /// rskj's `createP2pkhOutput`, `createP2shOutput` and `createBech32Output`.
    fn other_outputs(self) -> [Vec<u8>; 3] {
        let (p2pkh, p2sh, bech32) = match self {
            Net::Regtest => (
                "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn",
                "2N7TdMZSYfwFMUwyr8YADSKUhLUyH6eqohz",
                "0014ef57424d0d625cf82fabe4fd7657d24a5f13dfb2",
            ),
            Net::Mainnet => (
                "1JMaBRALrJQArLrqe5zRSn3bVk1z9RGzML",
                "3FiMaJinRy86WgJZDfAfRUt2ZLEBGZSfLL",
                "001437c383ea78269585c73289daa36d7b7014b65294",
            ),
        };
        [
            script_of(p2pkh),
            script_of(p2sh),
            alloy_primitives::hex::decode(bech32).unwrap(),
        ]
    }

    /// The first block at which a new federation made at block 0 is active,
    /// by its activation age before RSKIP383.
    fn block(self) -> u64 {
        self.constants().federation_activation_age_legacy
    }
}

fn p2pkh_script(hash: &[u8]) -> Vec<u8> {
    [&[0x76, 0xa9, 0x14][..], hash, &[0x88, 0xac]].concat()
}

fn p2sh_script(hash: &[u8]) -> Vec<u8> {
    [&[0xa9, 0x14][..], hash, &[0x87]].concat()
}

/// The output script that pays a base58 address.
fn script_of(address: &str) -> Vec<u8> {
    let bytes = bitcoin::base58::decode_check(address).unwrap();
    match bytes[0] {
        0 | 111 => p2pkh_script(&bytes[1..]),
        _ => p2sh_script(&bytes[1..]),
    }
}

/// rskj's `PegTestUtils.createFederation`: the keys of the private numbers,
/// sorted.
fn keys(privates: &[&str]) -> Vec<[u8; 33]> {
    let mut keys: Vec<[u8; 33]> = privates
        .iter()
        .map(|private| key_of_private(private))
        .collect();
    keys.sort();
    keys
}

/// rskj's tests' active federation, `fa03` to `fa05`.
fn active() -> Vec<[u8; 33]> {
    keys(&["fa03", "fa04", "fa05"])
}

/// rskj's tests' retiring federation, `fa01` and `fa02`.
fn retiring() -> Vec<[u8; 33]> {
    keys(&["fa01", "fa02"])
}

/// A standard multisig federation's redeem script: a majority of `keys`,
/// sorted as bitcoinj sorts them.
fn redeem_script(keys: &[[u8; 33]]) -> Vec<u8> {
    let mut keys = keys.to_vec();
    keys.sort();
    let mut script = vec![0x50 + (keys.len() / 2 + 1) as u8];
    for key in &keys {
        script.push(0x21);
        script.extend_from_slice(key);
    }
    script.extend_from_slice(&[0x50 + keys.len() as u8, 0xae]);
    script
}

/// Where a federation is stored and where its format is.
type Slot = (&'static str, &'static str);
const NEW: Slot = (NEW_FEDERATION_KEY, NEW_FEDERATION_FORMAT_VERSION_KEY);
const OLD: Slot = (OLD_FEDERATION_KEY, OLD_FEDERATION_FORMAT_VERSION_KEY);

/// A Bitcoin transaction paying `outputs`, with rskj's one input: output 0 of
/// the zero hash, its signature left empty, and the one-byte witness `push` if
/// there is one.
fn btc_tx(outputs: &[(u64, Vec<u8>)], push: Option<u8>) -> bitcoin::Transaction {
    bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::all_zeros(),
                vout: 0,
            },
            script_sig: bitcoin::ScriptBuf::from_bytes(
                [&[0x00, 0x21][..], &key_from_seed("input")].concat(),
            ),
            sequence: bitcoin::Sequence::MAX,
            witness: push.map_or_else(bitcoin::Witness::new, |push| {
                bitcoin::Witness::from_slice(&[[push]])
            }),
        }],
        output: outputs
            .iter()
            .map(|(value, script)| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(*value),
                script_pubkey: bitcoin::ScriptBuf::from_bytes(script.clone()),
            })
            .collect(),
    }
}

fn txid(tx: &bitcoin::Transaction) -> [u8; 32] {
    tx.compute_txid().to_byte_array()
}

fn wtxid(tx: &bitcoin::Transaction) -> [u8; 32] {
    tx.compute_wtxid().to_byte_array()
}

/// A partial merkle tree of one transaction, `hash`, matched.
fn one_leaf_tree(hash: &[u8; 32]) -> Vec<u8> {
    [&1u32.to_le_bytes()[..], &[1], hash, &[1, 0x3f]].concat()
}

fn wei(satoshis: u64) -> U256 {
    U256::from(satoshis) * U256::from(10_000_000_000u64)
}

/// A Flyover registration as rskj's tests set it up.
struct Flyover {
    bridge: Bridge,
    net: Net,
    args_hash: [u8; 32],
    user: Vec<u8>,
    lbc: Address,
    lp: Vec<u8>,
}

impl Flyover {
    /// `net` at `block` with every upgrade to `last`, called by the LBC.
    fn new(net: Net, last: RskNetworkUpgrade, block: u64) -> Self {
        let mut bridge = Bridge::new(up_to(last, net.chain_id()), net.constants(), block);
        bridge.caller = LBC;
        bridge.depth = 2;
        let address =
            |seed: &str| [vec![net.p2pkh()], hash160(&key_from_seed(seed)).to_vec()].concat();
        let mut args_hash = [0; 32];
        args_hash[0] = 1;
        Flyover {
            bridge,
            net,
            args_hash,
            user: address("refund"),
            lbc: LBC,
            lp: address("lp"),
        }
    }

    /// The Bridge holding all the bitcoin there is, so that none is locked.
    fn nothing_locked(&mut self) {
        self.bridge.credit(BRIDGE_ADDR, wei(2_100_000_000_000_000));
    }

    fn cap(&mut self, satoshis: u64) {
        bridge_store_u256(&mut self.bridge.ctx, LOCKING_CAP_KEY, U256::from(satoshis));
    }

    /// `keys` as a standard multisig federation made at block 0, in `slot`.
    fn store(&mut self, slot: Slot, keys: &[[u8; 33]]) {
        let (key, format_key) = slot;
        let members: Vec<StoredMember> = keys.iter().copied().map(StoredMember::from_btc).collect();
        bridge_store_bytes_named(&mut self.bridge.ctx, format_key, &rlp_encode_u64(1000));
        let stored = serialize_federation_multikey(&members, 1_000, 0);
        bridge_store_bytes_named(&mut self.bridge.ctx, key, &stored);
    }

    /// rskj's `PegUtils.getFlyoverDerivationHash`.
    fn derivation_hash(&self) -> [u8; 32] {
        use sha3::Digest;
        let data = [
            &self.args_hash[..],
            &self.user,
            self.lbc.as_slice(),
            &self.lp,
        ]
        .concat();
        sha3::Keccak256::digest(data).into()
    }

    /// The Flyover redeem script of `keys`' federation: the derivation hash
    /// pushed and dropped before the federation's redeem script.
    fn redeem(&self, keys: &[[u8; 33]]) -> Vec<u8> {
        [
            &[0x20][..],
            &self.derivation_hash(),
            &[0x75],
            &redeem_script(keys),
        ]
        .concat()
    }

    /// The output script of `keys`' federation's Flyover address, the P2SH of
    /// its Flyover redeem script.
    fn pays(&self, keys: &[[u8; 33]]) -> Vec<u8> {
        p2sh_script(&hash160(&self.redeem(keys)))
    }

    /// rskj's `mockChainOfStoredBlocks`: stored blocks from `height` up, the
    /// one at `height` with `merkle_root` and the network's confirmations
    /// above it, the last the chain's head. The hash of the one at `height`, in
    /// display order.
    fn chain(&mut self, height: u32, merkle_root: [u8; 32]) -> [u8; 32] {
        let use_v2 = self.bridge.cfg.has_stored_block_v2(self.bridge.number());
        let head = height
            + self
                .bridge
                .constants
                .btc2rsk_minimum_acceptable_confirmations;
        let mut prev = bitcoin::BlockHash::from_byte_array([2; 32]);
        let mut at_height = [0; 32];
        for at in height..=head {
            let root = if at == height {
                merkle_root
            } else {
                [0x11; 32]
            };
            let header = bitcoin::block::Header {
                version: bitcoin::block::Version::from_consensus(1),
                prev_blockhash: prev,
                merkle_root: bitcoin::TxMerkleNode::from_byte_array(root),
                time: 1,
                bits: bitcoin::CompactTarget::from_consensus(1),
                nonce: at,
            };
            let stored = StoredBlock::new(header, at, U256::from(at));
            put_stored_block(&mut self.bridge.ctx, &stored, use_v2);
            if at == head {
                store_chain_head(&mut self.bridge.ctx, &stored, use_v2);
            }
            if at == height {
                at_height = header.block_hash().to_byte_array();
                at_height.reverse();
            }
            prev = header.block_hash();
        }
        at_height
    }

    /// The Bridge's answer to the LBC's registration of `tx` at `height`.
    fn register(&mut self, tx: &[u8], height: i64, pmt: &[u8], to_lp: bool) -> Vec<u8> {
        let data = call_data(
            SIGNATURE,
            &[
                Arg::Bytes(tx),
                int(height),
                Arg::Bytes(pmt),
                Arg::Word(self.args_hash),
                Arg::Bytes(&self.user),
                address(self.lbc),
                Arg::Bytes(&self.lp),
                boolean(to_lp),
            ],
        );
        self.bridge.answer(&data)
    }

    /// `tx` in the block at height 1 of a chain stored for it, registered
    /// there.
    fn register_in_chain(&mut self, tx: &bitcoin::Transaction, to_lp: bool) -> Vec<u8> {
        self.chain(1, txid(tx));
        let raw = bitcoin::consensus::serialize(tx);
        self.register(&raw, 1, &one_leaf_tree(&wtxid(tx)), to_lp)
    }

    fn balance_of_the_lbc(&mut self) -> U256 {
        self.bridge.balance(LBC)
    }

    /// The peg-outs waiting for confirmations.
    fn waiting_for_confirmations(&mut self) -> Vec<bitcoin::Transaction> {
        let with_hash = self.bridge.cfg.has_rskip146(self.bridge.number());
        crate::bridge::peg::load_pegout_confirmation_set(&mut self.bridge.ctx, with_hash)
            .iter()
            .map(|entry| bitcoin::consensus::deserialize(&entry.btc_tx_raw).unwrap())
            .collect()
    }
}

/// rskj's `sendFundsToAnyAddress`: the federations `active` as the new one
/// and `retiring` as the old, a payment with `outputs`, given what pays the
/// active federation's Flyover address and the retiring one's.
fn send_to_any_address(
    net: Net,
    rskip293: bool,
    outputs: impl Fn(&[u8], &[u8]) -> Vec<(u64, Vec<u8>)>,
) -> Vec<u8> {
    let last = if rskip293 { Hop400 } else { Iris300 };
    let mut flyover = Flyover::new(net, last, net.block());
    flyover.nothing_locked();
    flyover.store(NEW, &active());
    flyover.store(OLD, &retiring());
    let tx = btc_tx(
        &outputs(&flyover.pays(&active()), &flyover.pays(&retiring())),
        None,
    );
    flyover.register_in_chain(&tx, true)
}

/// rskj's `registerFlyoverBtcTransaction_output_to_bech32_before_RSKIP293_*`
/// and `…multiple_output_to_all_address_type_before_RSKIP293_*`: before
/// RSKIP293 rskj reads every output of the payment as an address
/// (`BridgeUtilsLegacy.getAmountSentToAddress`, which calls bitcoinj's
/// `Script.getToAddress` on each), and bitcoinj throws a `ScriptException`
/// for any but P2PKH and P2SH. The Bridge's method answers the throw with
/// GENERIC_ERROR.
#[test]
fn before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error() {
    for net in [Net::Regtest, Net::Mainnet] {
        let [p2pkh, p2sh, bech32] = net.other_outputs();
        let bech32_alone = send_to_any_address(net, false, |_, _| vec![(COIN, bech32.clone())]);
        assert_eq!(int_of(&bech32_alone), GENERIC_ERROR, "{net:?}");
        let every_type = send_to_any_address(net, false, |active, _| {
            vec![
                (COIN, active.to_vec()),
                (COIN, bech32.clone()),
                (COIN, p2pkh.clone()),
                (COIN, p2sh.clone()),
            ]
        });
        assert_eq!(int_of(&every_type), GENERIC_ERROR, "{net:?}");
    }
}

/// rskj's `registerFlyoverBtcTransaction_output_to_bech32_after_RSKIP293_*` and
/// `…multiple_output_to_all_address_type_after_RSKIP293_*`: from RSKIP293 rskj
/// asks a wallet which outputs are the federations', and what pays
/// elsewhere, in whatever script, is passed over.
#[test]
fn from_rskip293_outputs_to_other_addresses_are_passed_over() {
    for net in [Net::Regtest, Net::Mainnet] {
        let [p2pkh, p2sh, bech32] = net.other_outputs();
        let with_bech32 = send_to_any_address(net, true, |active, _| {
            vec![(COIN, active.to_vec()), (COIN, bech32.clone())]
        });
        assert_eq!(uint_of(&with_bech32), wei(COIN), "{net:?}");
        let every_type = send_to_any_address(net, true, |active, _| {
            vec![
                (COIN, active.to_vec()),
                (COIN, bech32.clone()),
                (COIN, p2pkh.clone()),
                (COIN, p2sh.clone()),
            ]
        });
        assert_eq!(uint_of(&every_type), wei(COIN), "{net:?}");
    }
}

/// From RSKIP293 the wallet reads an output through bitcoinj-thin's
/// `TransactionOutput.isMine` and `isWatched`, which catch the
/// `ScriptException` of a script that doesn't parse and pass the output over:
/// a payment with such an output beside the one to the active federation's
/// Flyover address is registered for what it sends there. Before RSKIP293
/// rskj reads that output as an address, bitcoinj throws, and the method
/// answers GENERIC_ERROR. The outputs that don't parse: an OP_PUSHDATA1 with
/// no length after it, and a push of two bytes with one.
#[test]
fn from_rskip293_an_output_whose_script_does_not_parse_is_passed_over() {
    for net in [Net::Regtest, Net::Mainnet] {
        for unparseable in [vec![0x4c], vec![0x02, 0x01]] {
            let outputs = |active: &[u8], _: &[u8]| {
                vec![(COIN, active.to_vec()), (COIN, unparseable.clone())]
            };
            let registered = send_to_any_address(net, true, outputs);
            assert_eq!(
                uint_of(&registered),
                wei(COIN),
                "{net:?} {unparseable:02x?}"
            );
            let before = send_to_any_address(net, false, outputs);
            assert_eq!(int_of(&before), GENERIC_ERROR, "{net:?} {unparseable:02x?}");
        }
    }
}

/// rskj's `registerFlyoverBtcTransaction_multiple_output_to_random_addresses_*`:
/// a payment to other addresses alone sends the federations nothing.
#[test]
fn a_payment_to_other_addresses_alone_is_of_no_value() {
    for net in [Net::Regtest, Net::Mainnet] {
        let [p2pkh, p2sh, _] = net.other_outputs();
        let answer = send_to_any_address(net, true, |_, _| {
            vec![
                (COIN, p2pkh.clone()),
                (COIN, p2pkh.clone()),
                (COIN, p2pkh.clone()),
                (COIN, p2sh.clone()),
                (COIN, p2sh.clone()),
            ]
        });
        assert_eq!(int_of(&answer), VALUE_ZERO, "{net:?}");
    }
}

/// rskj's `registerFlyoverBtcTransaction_output_to_P2PKH_*`, `…output_to_P2SH_*`,
/// `…output_to_different_address_type_*`, `…multiple_output_to_the_same_address_*`
/// and `…multiple_output_to_different_addresses_*`: from RSKIP293 each output
/// to either federation's Flyover address counts, and nothing else does. The
/// outputs, a bitcoin each: `a` to the active federation, `r` to the retiring
/// one, `k` to a P2PKH address, `s` to a P2SH one.
#[test]
fn from_rskip293_what_pays_either_federation_counts_and_nothing_else() {
    for net in [Net::Regtest, Net::Mainnet] {
        let [p2pkh, p2sh, _] = net.other_outputs();
        for (outputs, coins) in [
            ("ak", 1),
            ("as", 1),
            ("arsk", 2),
            ("aaaa", 4),
            ("aarrrkkss", 5),
        ] {
            let answer = send_to_any_address(net, true, |active, retiring| {
                outputs
                    .chars()
                    .map(|to| match to {
                        'a' => (COIN, active.to_vec()),
                        'r' => (COIN, retiring.to_vec()),
                        'k' => (COIN, p2pkh.clone()),
                        _ => (COIN, p2sh.clone()),
                    })
                    .collect()
            });
            assert_eq!(uint_of(&answer), wei(coins * COIN), "{net:?} {outputs}");
        }
    }
}

/// rskj's `registerFlyoverBtcTransaction_sent_multiple_output_and_one_with_amount_below_minimum_*`:
/// from RSKIP293, one output to a federation a satoshi short of the minimum
/// peg-in turns the whole payment down, the others at or past it.
#[test]
fn from_rskip293_one_output_below_the_minimum_turns_the_payment_down() {
    for net in [Net::Regtest, Net::Mainnet] {
        let minimum = net.constants().minimum_pegin_tx_value;
        let [p2pkh, _, _] = net.other_outputs();
        let answer = send_to_any_address(net, true, |active, retiring| {
            vec![
                (minimum, p2pkh.clone()),
                (COIN, p2pkh.clone()),
                (COIN, p2pkh.clone()),
                (minimum, active.to_vec()),
                (minimum - 1, active.to_vec()),
                (minimum + 1, retiring.to_vec()),
                (minimum + COIN, retiring.to_vec()),
            ]
        });
        assert_eq!(int_of(&answer), UTXO_BELOW_MINIMUM, "{net:?}");
    }
}

/// rskj's `sendFundsToActiveFederation`: only the new federation, `active`,
/// stored, and `value` sent to its Flyover address. On success the LBC is
/// paid the value.
fn send_to_the_active_federation(rskip293: bool, value: u64) -> Vec<u8> {
    let last = if rskip293 { Hop400 } else { Iris300 };
    let mut flyover = Flyover::new(Net::Regtest, last, Net::Regtest.block());
    flyover.nothing_locked();
    flyover.store(NEW, &active());
    let tx = btc_tx(&[(value, flyover.pays(&active()))], None);
    let before = flyover.balance_of_the_lbc();
    let answer = flyover.register_in_chain(&tx, true);
    if int_of(&answer) > 0 {
        assert_eq!(flyover.balance_of_the_lbc(), before + wei(value));
    }
    answer
}

/// rskj's `registerFlyoverBtcTransaction_amount_sent_is_below_minimum_before_RSKIP293_activation`.
#[test]
fn before_rskip293_any_amount_to_the_active_federation_is_taken() {
    let half = Net::Regtest.constants().minimum_pegin_tx_value / 2;
    assert_eq!(
        uint_of(&send_to_the_active_federation(false, half)),
        wei(half)
    );
}

/// rskj's `registerFlyoverBtcTransaction_amount_sent_is_below_minimum_after_RSKIP293_activation`,
/// `…amount_sent_is_equal_to_minimum` and `…amount_sent_is_over_minimum`.
#[test]
fn from_rskip293_a_payment_must_reach_the_minimum_peg_in() {
    let minimum = Net::Regtest.constants().minimum_pegin_tx_value;
    assert_eq!(
        int_of(&send_to_the_active_federation(true, minimum / 2)),
        UTXO_BELOW_MINIMUM
    );
    assert_eq!(
        uint_of(&send_to_the_active_federation(true, minimum)),
        wei(minimum)
    );
    let over = minimum + CENT;
    assert_eq!(
        uint_of(&send_to_the_active_federation(true, over)),
        wei(over)
    );
}

/// rskj's `registerFlyoverBtcTransaction_funds_sent_to_active_fed_after_RSKIP293`.
#[test]
fn from_rskip293_a_payment_to_the_active_federation_is_taken() {
    assert_eq!(
        uint_of(&send_to_the_active_federation(true, COIN)),
        wei(COIN)
    );
}

/// rskj's `sendFundsToRetiringFederation` and
/// `sendFundsToActiveAndRetiringFederation`: what a payment of a bitcoin to
/// the retiring federation, and to both, is answered.
fn send_to_the_retiring_federation(rskip293: bool) -> (Vec<u8>, Vec<u8>) {
    let to = |both: bool| {
        send_to_any_address(Net::Regtest, rskip293, |active, retiring| {
            let mut outputs = vec![(COIN, retiring.to_vec())];
            if both {
                outputs.insert(0, (COIN, active.to_vec()));
            }
            outputs
        })
    };
    (to(false), to(true))
}

/// rskj's `registerFlyoverBtcTransaction_funds_sent_to_current_retiring_fed_before_RSKIP293_activation`
/// and `…funds_sent_to_active_and_retiring_fed_before_RSKIP293_activation`.
#[test]
fn before_rskip293_what_pays_the_retiring_federation_does_not_count() {
    let (retiring, both) = send_to_the_retiring_federation(false);
    assert_eq!(int_of(&retiring), VALUE_ZERO);
    assert_eq!(uint_of(&both), wei(COIN));
}

/// rskj's `registerFlyoverBtcTransaction_funds_sent_to_retiring_fed_after_RSKIP293`
/// and `…funds_sent_to_active_and_retiring_fed_after_RSKIP293_activation`.
#[test]
fn from_rskip293_what_pays_the_retiring_federation_counts() {
    let (retiring, both) = send_to_the_retiring_federation(true);
    assert_eq!(uint_of(&retiring), wei(COIN));
    assert_eq!(uint_of(&both), wei(2 * COIN));
}

/// rskj's `sendFundsSurpassesLockingCapToAnyAddress`: a cap of 50 bitcoin,
/// none locked, the federations `active` as the new one and `retiring` as the
/// old. A refund is one peg-out waiting for confirmations, of one output to
/// the LP or the user, worth less than what the federations were sent and
/// more than nine tenths of it.
fn send_over_the_cap(
    rskip293: bool,
    outputs: impl Fn(&[u8], &[u8]) -> Vec<(u64, Vec<u8>)>,
    to_lp: bool,
) -> Vec<u8> {
    let last = if rskip293 { Hop400 } else { Iris300 };
    let mut flyover = Flyover::new(Net::Regtest, last, Net::Regtest.block());
    flyover.nothing_locked();
    flyover.cap(50 * COIN);
    flyover.store(NEW, &active());
    flyover.store(OLD, &retiring());
    let (to_active, to_retiring) = (flyover.pays(&active()), flyover.pays(&retiring()));
    let tx = btc_tx(&outputs(&to_active, &to_retiring), None);
    let answer = flyover.register_in_chain(&tx, to_lp);
    if [REFUNDED_LP, REFUNDED_USER].contains(&int_of(&answer)) {
        let counted = |script: &bitcoin::ScriptBuf| {
            script.as_bytes() == to_active || (rskip293 && script.as_bytes() == to_retiring)
        };
        let sent: u64 = tx
            .output
            .iter()
            .filter(|output| counted(&output.script_pubkey))
            .map(|output| output.value.to_sat())
            .sum();
        let refund = flyover.waiting_for_confirmations();
        assert_eq!(refund.len(), 1);
        assert_eq!(refund[0].output.len(), 1);
        let to = if to_lp { &flyover.lp } else { &flyover.user };
        assert_eq!(
            refund[0].output[0].script_pubkey.as_bytes(),
            p2pkh_script(&to[1..])
        );
        let refunded = refund[0].output[0].value.to_sat();
        assert!(
            refunded > sent - sent / 10 && refunded < sent,
            "{refunded} of {sent}"
        );
    }
    answer
}

/// rskj's `registerFlyoverBtcTransaction_sum_of_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_before_RSKIP293_no_refund`
/// and the two `…before_RSKIP293_only_funds_sent_to_active_fed_should_be_refunded_to_*`:
/// before RSKIP293 only what pays the active federation counts against the
/// cap, and is what a refund returns.
#[test]
fn before_rskip293_only_the_active_federations_part_counts_against_the_cap() {
    let under = send_over_the_cap(
        false,
        |active, retiring| vec![(25 * COIN, active.to_vec()), (50 * COIN, retiring.to_vec())],
        true,
    );
    assert_eq!(uint_of(&under), wei(25 * COIN));
    for (to_lp, refunded) in [(true, REFUNDED_LP), (false, REFUNDED_USER)] {
        let over = send_over_the_cap(
            false,
            |active, retiring| vec![(51 * COIN, active.to_vec()), (51 * COIN, retiring.to_vec())],
            to_lp,
        );
        assert_eq!(int_of(&over), refunded);
    }
}

/// rskj's `registerFlyoverBtcTransaction_funds_sent_to_retiring_fed_surpasses_locking_cap_should_refund_user_after_RSKIP293`,
/// `…sum_of_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_should_refund_lp_after_RSKIP293`,
/// `…sum_of_all_utxos_surpass_locking_cap_but_not_funds_sent_to_fed_should_not_refund_after_RSKIP293`
/// and `…funds_sent_to_random_addresses_surpass_locking_cap_should_not_refund_after_RSKIP293`:
/// from RSKIP293 what pays either federation counts against the cap, and
/// what pays elsewhere doesn't.
#[test]
fn from_rskip293_both_federations_parts_count_against_the_cap() {
    let retiring_over = send_over_the_cap(
        true,
        |_, retiring| vec![(51 * COIN, retiring.to_vec())],
        false,
    );
    assert_eq!(int_of(&retiring_over), REFUNDED_USER);
    let both_over = send_over_the_cap(
        true,
        |active, retiring| vec![(50 * COIN, active.to_vec()), (50 * COIN, retiring.to_vec())],
        true,
    );
    assert_eq!(int_of(&both_over), REFUNDED_LP);
    let elsewhere = script_of("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn");
    let mostly_elsewhere = send_over_the_cap(
        true,
        |active, retiring| {
            vec![
                (COIN, active.to_vec()),
                (COIN, retiring.to_vec()),
                (50 * COIN, elsewhere.clone()),
            ]
        },
        false,
    );
    assert_eq!(uint_of(&mostly_elsewhere), wei(2 * COIN));
    let all_elsewhere = send_over_the_cap(
        true,
        |_, _| {
            vec![
                (50 * COIN, elsewhere.clone()),
                (COIN, p2sh_script(&[0x22; 20])),
            ]
        },
        false,
    );
    assert_eq!(int_of(&all_elsewhere), VALUE_ZERO);
}

/// The witness tests' set-up: regtest at iris300, only the new federation,
/// `active`, stored, a payment of `value` to its Flyover address, and, below
/// that, a cap of `cap` where one is given.
fn witness_flyover(value: u64, cap: Option<u64>) -> (Flyover, bitcoin::Transaction) {
    let mut flyover = Flyover::new(Net::Regtest, Iris300, Net::Regtest.block());
    flyover.nothing_locked();
    if let Some(cap) = cap {
        flyover.cap(cap);
    }
    flyover.store(NEW, &active());
    let tx = btc_tx(&[(value, flyover.pays(&active()))], None);
    (flyover, tx)
}

/// `tx` with rskj's one-byte witness, in the block at height 1 of a chain
/// whose coinbase registers `witness_root` (display order); its answer when
/// proven by `wtxid`'s tree.
fn register_with_a_witness(
    flyover: &mut Flyover,
    tx: &bitcoin::Transaction,
    witness_root: [u8; 32],
) -> Vec<u8> {
    let mut with = tx.clone();
    with.input[0].witness = bitcoin::Witness::from_slice(&[[1u8]]);
    let block = flyover.chain(1, txid(&with));
    crate::bridge::tx::set_coinbase_information(&mut flyover.bridge.ctx, &block, &witness_root);
    let raw = bitcoin::consensus::serialize(&with);
    flyover.register(&raw, 1, &one_leaf_tree(&wtxid(&with)), false)
}

fn reversed(mut hash: [u8; 32]) -> [u8; 32] {
    hash.reverse();
    hash
}

fn witness_root_of(tx: &bitcoin::Transaction) -> [u8; 32] {
    let mut with = tx.clone();
    with.input[0].witness = bitcoin::Witness::from_slice(&[[1u8]]);
    reversed(wtxid(&with))
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_with_witness_already_saved_in_storage`:
/// a payment with a witness, proven by its wtxid against the coinbase's
/// witness root, is registered once.
#[test]
fn a_payment_with_a_witness_is_registered_once() {
    let (mut flyover, tx) = witness_flyover(COIN, None);
    let root = witness_root_of(&tx);
    assert_eq!(
        uint_of(&register_with_a_witness(&mut flyover, &tx, root)),
        wei(COIN)
    );
    assert_eq!(
        int_of(&register_with_a_witness(&mut flyover, &tx, root)),
        ALREADY_PROCESSED
    );
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_with_witness_surpassing_locking_already_saved_in_storage`.
#[test]
fn a_refunded_payment_with_a_witness_is_not_registered_again() {
    let (mut flyover, tx) = witness_flyover(2 * COIN, Some(COIN));
    let root = witness_root_of(&tx);
    assert_eq!(
        int_of(&register_with_a_witness(&mut flyover, &tx, root)),
        REFUNDED_USER
    );
    assert_eq!(
        int_of(&register_with_a_witness(&mut flyover, &tx, root)),
        ALREADY_PROCESSED
    );
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_with_witness_surpassing_locking_already_saved_in_storage_and_then_send_same_without_witness`:
/// what a refund marks is the payment's txid, so the payment without its
/// witness is turned down too, though the cap would now take it.
#[test]
fn a_payment_refunded_with_a_witness_is_not_registered_without_it() {
    let (mut flyover, tx) = witness_flyover(2 * COIN, Some(COIN));
    let root = witness_root_of(&tx);
    assert_eq!(
        int_of(&register_with_a_witness(&mut flyover, &tx, root)),
        REFUNDED_USER
    );
    flyover.cap(3 * COIN);
    let raw = bitcoin::consensus::serialize(&tx);
    let answer = flyover.register(&raw, 1, &one_leaf_tree(&txid(&tx)), false);
    assert_eq!(int_of(&answer), ALREADY_PROCESSED);
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_without_witness_surpassing_locking_already_saved_in_storage`.
#[test]
fn a_refunded_payment_is_not_registered_again() {
    let (mut flyover, tx) = witness_flyover(2 * COIN, Some(COIN));
    assert_eq!(
        int_of(&flyover.register_in_chain(&tx, false)),
        REFUNDED_USER
    );
    assert_eq!(
        int_of(&flyover.register_in_chain(&tx, false)),
        ALREADY_PROCESSED
    );
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_without_witness_surpassing_locking_already_saved_in_storage_and_then_send_same_tx_with_witness`:
/// the payment again with a witness, proven by its wtxid where the coinbase
/// registers the root of the tree without it, as rskj's test registers it,
/// fails the validations.
#[test]
fn a_refunded_payment_again_with_a_witness_fails_the_validations() {
    let (mut flyover, tx) = witness_flyover(2 * COIN, Some(COIN));
    assert_eq!(
        int_of(&flyover.register_in_chain(&tx, false)),
        REFUNDED_USER
    );
    let answer = register_with_a_witness(&mut flyover, &tx, reversed(txid(&tx)));
    assert_eq!(int_of(&answer), VALIDATIONS);
}

/// rskj's `registerFlyoverBtcTransaction_failed_when_tx_without_witness_already_saved_in_storage`.
#[test]
fn a_payment_is_registered_once() {
    let (mut flyover, tx) = witness_flyover(COIN, None);
    assert_eq!(uint_of(&flyover.register_in_chain(&tx, false)), wei(COIN));
    assert_eq!(
        int_of(&flyover.register_in_chain(&tx, false)),
        ALREADY_PROCESSED
    );
}

/// rskj's `registerFlyoverBtcTransaction_is_not_contract`: a call that isn't
/// a contract's is turned down first.
#[test]
fn a_registration_not_from_a_contract_is_turned_down() {
    let mut flyover = Flyover::new(Net::Regtest, Iris300, Net::Regtest.block());
    flyover.bridge.depth = 1;
    assert_eq!(int_of(&flyover.register(&[], 0, &[], false)), NOT_CONTRACT);
}

/// rskj's `registerFlyoverBtcTransaction_sender_is_not_lbc`: a contract
/// other than the LBC the call names is turned down.
#[test]
fn a_registration_from_another_contract_than_the_lbc_is_turned_down() {
    let mut flyover = Flyover::new(Net::Regtest, Iris300, Net::Regtest.block());
    flyover.bridge.caller = Address::repeat_byte(0x01);
    // rskj's `new BtcTransaction(params).bitcoinSerialize()`: version 1, no
    // inputs, no outputs, lock time 0.
    let empty = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        int_of(&flyover.register(&empty, 100, &[0xab], false)),
        INVALID_SENDER
    );
}

/// rskj's `registerFlyoverBtcTransaction_validationsForRegisterBtcTransaction_returns_false`:
/// with the chain at regtest's genesis, a payment at height 100 has no
/// confirmations.
#[test]
fn a_payment_the_chain_cannot_confirm_fails_the_validations() {
    let mut flyover = Flyover::new(Net::Regtest, Iris300, Net::Regtest.block());
    flyover.store(NEW, &active());
    let tx = btc_tx(&[(COIN, flyover.pays(&active()))], None);
    let raw = bitcoin::consensus::serialize(&tx);
    assert_eq!(
        int_of(&flyover.register(&raw, 100, &[0xab], false)),
        VALIDATIONS
    );
}

/// rskj's tests' `getGenesisFederationLegacy`: `net`'s genesis keys as a
/// standard multisig federation, stored as the new one, and, as those tests
/// name them, a zero arguments' hash and one address for both the user and
/// the LP, `n3PLxDiwWqa5uH7fSbHCxS6VAjD9Y7Rwkj`'s key hash. A payment of
/// `value` to the federation's Flyover address, in the block at height 100 of
/// a chain stored for it.
fn pay_the_genesis_federation(net: Net, value: u64) -> (Flyover, Vec<u8>, Vec<u8>) {
    let mut flyover = Flyover::new(net, Iris300, net.block());
    let genesis = crate::bridge::peg::genesis_federation_keys(&flyover.bridge.constants);
    flyover.store(NEW, &genesis);
    flyover.args_hash = [0; 32];
    let address = bitcoin::base58::decode_check("n3PLxDiwWqa5uH7fSbHCxS6VAjD9Y7Rwkj").unwrap();
    flyover.user = [&[flyover.net.p2pkh()][..], &address[1..]].concat();
    flyover.lp = flyover.user.clone();
    let tx = btc_tx(&[(value, flyover.pays(&genesis))], None);
    flyover.chain(100, txid(&tx));
    (
        flyover,
        bitcoin::consensus::serialize(&tx),
        one_leaf_tree(&txid(&tx)),
    )
}

/// rskj's `registerFlyoverBtcTransaction_amount_sent_is_0`.
#[test]
fn a_payment_of_nothing_is_of_no_value() {
    let (mut flyover, tx, pmt) = pay_the_genesis_federation(Net::Mainnet, 0);
    flyover.nothing_locked();
    assert_eq!(int_of(&flyover.register(&tx, 100, &pmt, false)), VALUE_ZERO);
}

/// rskj's `registerFlyoverBtcTransaction_surpasses_locking_cap_and_shouldTransfer_is_true`
/// and `…_is_false`: with a cap of a bitcoin and a Bridge of one wei, all
/// but nothing is locked, and a bitcoin is refunded to the LP or the user.
#[test]
fn over_the_cap_a_payment_is_refunded_to_the_lp_or_the_user() {
    for (to_lp, refunded) in [(true, REFUNDED_LP), (false, REFUNDED_USER)] {
        let (mut flyover, tx, pmt) = pay_the_genesis_federation(Net::Regtest, COIN);
        flyover.bridge.credit(BRIDGE_ADDR, U256::from(1));
        flyover.cap(COIN);
        assert_eq!(int_of(&flyover.register(&tx, 100, &pmt, to_lp)), refunded);
    }
}

/// rskj's `registerFlyoverBtcTransaction_surpasses_locking_cap_and_tries_to_register_again`.
#[test]
fn a_payment_refunded_over_the_cap_is_not_registered_again() {
    let (mut flyover, tx, pmt) = pay_the_genesis_federation(Net::Regtest, COIN);
    flyover.bridge.credit(BRIDGE_ADDR, U256::from(1));
    flyover.cap(COIN);
    assert_eq!(
        int_of(&flyover.register(&tx, 100, &pmt, false)),
        REFUNDED_USER
    );
    flyover.cap(50 * COIN);
    assert_eq!(
        int_of(&flyover.register(&tx, 100, &pmt, false)),
        ALREADY_PROCESSED
    );
}

/// rskj's `registerFlyoverBtcTransaction_OK` and `saveFlyoverDataInStorage_OK`:
/// a payment is registered, the LBC is paid its value, the federation gets
/// its output, the Flyover address's information is stored under the hash of
/// its redeem script, and the payment is not registered again.
#[test]
fn a_payment_is_registered_and_its_value_goes_to_the_lbc() {
    let (mut flyover, tx, pmt) = pay_the_genesis_federation(Net::Regtest, COIN);
    flyover.nothing_locked();
    let genesis = crate::bridge::peg::genesis_federation_keys(&flyover.bridge.constants);
    let before = flyover.balance_of_the_lbc();
    assert_eq!(uint_of(&flyover.register(&tx, 100, &pmt, true)), wei(COIN));
    assert_eq!(flyover.balance_of_the_lbc(), before + wei(COIN));
    let number = flyover.bridge.number();
    let cfg = flyover.bridge.cfg.clone();
    let utxos =
        crate::bridge::storage::load_federation_utxos(&mut flyover.bridge.ctx, &cfg, number);
    assert_eq!(utxos.len(), 1);
    assert_eq!((utxos[0].vout, utxos[0].value_satoshis), (0, COIN));
    assert_eq!(utxos[0].script, flyover.pays(&genesis));
    let redeem = flyover.redeem(&genesis);
    let information = compound_key(
        FAST_BRIDGE_FEDERATION_INFO_KEY,
        "-",
        &alloy_primitives::hex::encode(hash160(&redeem)),
    );
    let expected = rlp_encode_list(&[
        rlp_encode_element(&flyover.derivation_hash()),
        rlp_encode_element(&hash160(&redeem_script(&genesis))),
    ]);
    assert_eq!(
        bridge_load_raw(&mut flyover.bridge.ctx, information),
        Some(expected)
    );
    assert_eq!(
        int_of(&flyover.register(&tx, 100, &pmt, true)),
        ALREADY_PROCESSED
    );
}

/// rskj's `getFlyoverDerivationHash_ok`: from RSKIP284 the derivation hash is
/// the Keccak-256 of the arguments' hash, the user's refund address, the LBC
/// and the LP's address, each address its version and its key hash. A
/// payment to the Flyover address of that hash, with rskj's test's
/// arguments' hash, addresses and LBC, is taken.
#[test]
fn the_derivation_hash_is_of_the_arguments_the_addresses_and_the_lbc() {
    let mut flyover = Flyover::new(Net::Regtest, Hop400, Net::Regtest.block());
    flyover.nothing_locked();
    flyover.store(NEW, &active());
    flyover.args_hash = [0; 32];
    flyover.args_hash[31] = 1;
    flyover.user = bitcoin::base58::decode_check("mgy8yiUZYB7o9vvCu2Yi8GB3Vr32MQsyQJ").unwrap();
    flyover.lp = bitcoin::base58::decode_check("mhoDGMzHHDq2ZD6cFrKV9USnMfpxEtLwGm").unwrap();
    flyover.lbc = Address::with_last_byte(3);
    flyover.bridge.caller = flyover.lbc;
    let tx = btc_tx(&[(COIN, flyover.pays(&active()))], None);
    assert_eq!(uint_of(&flyover.register_in_chain(&tx, true)), wei(COIN));
}

/// rskj's nested `FlyoverPeginThatSurpassesLockingCap`: on mainnet, a payment
/// to the active federation of a satoshi over the initial cap, in a block
/// past where the peg-out index is read. The answer, the refund, and the
/// Bridge with it.
fn over_the_initial_cap(
    last: RskNetworkUpgrade,
    to_lp: bool,
) -> (i64, Flyover, bitcoin::Transaction) {
    let mut flyover = Flyover::new(Net::Mainnet, last, 1);
    flyover.store(NEW, &active());
    let amount = flyover.bridge.constants.initial_locking_cap + 1;
    let tx = btc_tx(&[(amount, flyover.pays(&active()))], None);
    let constants = &flyover.bridge.constants;
    let height = constants.btc_height_when_pegout_tx_index_activates
        + constants.pegout_tx_index_grace_period_in_btc_blocks;
    flyover.chain(height, txid(&tx));
    let raw = bitcoin::consensus::serialize(&tx);
    let answer = flyover.register(&raw, i64::from(height), &one_leaf_tree(&txid(&tx)), to_lp);
    let mut refund = flyover.waiting_for_confirmations();
    assert_eq!(refund.len(), 1);
    (int_of(&answer), flyover, refund.remove(0))
}

/// `pegout_transaction_created` for `refund`, with the value of the output
/// it spends: in the Bridge's logs.
fn logs_pegout_transaction_created(
    flyover: &Flyover,
    refund: &bitcoin::Transaction,
    spent: u64,
) -> bool {
    use revm::context_interface::{ContextTr, JournalTr};
    use sha3::Digest;
    let topic: [u8; 32] =
        sha3::Keccak256::digest("pegout_transaction_created(bytes32,bytes)").into();
    let topics = [B256::from(topic), B256::from(reversed(txid(refund)))];
    let varint = [&[0xff][..], &spent.to_le_bytes()].concat();
    let data = encode::args(&[Arg::Bytes(&varint)]);
    flyover
        .bridge
        .ctx
        .journal()
        .logs()
        .iter()
        .any(|log| log.topics() == topics.as_slice() && log.data.data.as_ref() == data.as_slice())
}

/// The outpoint values stored for `refund`.
fn stored_outpoint_values(flyover: &mut Flyover, refund: &bitcoin::Transaction) -> Option<Vec<u8>> {
    let hash = alloy_primitives::hex::encode(reversed(txid(refund)));
    let key = compound_key(RELEASES_OUTPOINTS_VALUES_KEY, "-", &hash);
    bridge_load_raw(&mut flyover.bridge.ctx, key)
}

/// rskj's `registerFlyoverBtcTransaction_whenAmountRequestedSurpassesLockingCap_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent`:
/// at lovell700 the refund is logged with the value it spends, which is not
/// stored.
#[test]
fn before_rskip305_a_refund_over_the_cap_is_logged_and_its_values_not_stored() {
    for (to_lp, refunded) in [(true, REFUNDED_LP), (false, REFUNDED_USER)] {
        let (answer, mut flyover, refund) = over_the_initial_cap(Lovell700, to_lp);
        assert_eq!(answer, refunded);
        let spent = flyover.bridge.constants.initial_locking_cap + 1;
        assert!(logs_pegout_transaction_created(&flyover, &refund, spent));
        assert_eq!(stored_outpoint_values(&mut flyover, &refund), None);
    }
}

/// rskj's `registerFlyoverBtcTransaction_whenAmountRequestedSurpassesLockingCap_afterRSKIP305_processReleaseRejectionTransactionInfo`:
/// with every upgrade the refund is logged and the value it spends stored.
#[test]
fn from_rskip305_a_refund_over_the_cap_is_logged_and_its_values_stored() {
    for (to_lp, refunded) in [(true, REFUNDED_LP), (false, REFUNDED_USER)] {
        let (answer, mut flyover, refund) = over_the_initial_cap(Vetiver900, to_lp);
        assert_eq!(answer, refunded);
        let spent = flyover.bridge.constants.initial_locking_cap + 1;
        assert!(logs_pegout_transaction_created(&flyover, &refund, spent));
        let varint = [&[0xff][..], &spent.to_le_bytes()].concat();
        assert_eq!(stored_outpoint_values(&mut flyover, &refund), Some(varint));
    }
}

/// rskj's `FederationFormatVersion.P2SH_ERP_FEDERATION`.
const P2SH_ERP: u64 = 3000;
/// rskj's `FederationFormatVersion.P2SH_P2WSH_ERP_FEDERATION`.
const P2SH_P2WSH_ERP: u64 = 4000;

/// The keys of the seeds `prefix` and 01 up to `count`, sorted, as rskj's
/// `P2shErpFederationBuilder` and `P2shP2wshErpFederationBuilder` take them.
fn seeded_keys(prefix: &str, count: usize) -> Vec<[u8; 33]> {
    let mut keys: Vec<[u8; 33]> = (1..=count)
        .map(|n| key_from_seed(&format!("{prefix}{n:02}")))
        .collect();
    keys.sort();
    keys
}

impl Flyover {
    /// `keys` as a federation of `format` made at block 1, in `slot`, as
    /// rskj's federation builders make one.
    fn store_as(&mut self, slot: Slot, keys: &[[u8; 33]], format: u64) {
        let (key, format_key) = slot;
        let members: Vec<StoredMember> = keys.iter().copied().map(StoredMember::from_btc).collect();
        bridge_store_bytes_named(&mut self.bridge.ctx, format_key, &rlp_encode_u64(format));
        let stored = serialize_federation_multikey(&members, 100_000_000_000, 1);
        bridge_store_bytes_named(&mut self.bridge.ctx, key, &stored);
    }

    /// The Flyover redeem script of the federation of `keys` and `format`.
    fn redeem_of(&self, keys: &[[u8; 33]], format: u64) -> Vec<u8> {
        let federation = crate::bridge::peg::federation_redeem_for_format_pub(
            keys,
            format,
            &self.bridge.constants,
            &self.bridge.cfg,
            self.bridge.number(),
        );
        [&[0x20][..], &self.derivation_hash(), &[0x75], &federation].concat()
    }

    /// The output script of that federation's Flyover address: the P2SH of
    /// its Flyover redeem script, or of that script's P2WSH program where the
    /// federation is P2SH-P2WSH.
    fn pays_to(&self, keys: &[[u8; 33]], format: u64) -> Vec<u8> {
        let redeem = self.redeem_of(keys, format);
        p2sh_script(&crate::bridge::peg::federation_output_hash160(
            &redeem, format,
        ))
    }
}

/// rskj's `FlyoverPeginThatSurpassesLockingCap` with both federations paid:
/// on mainnet with every upgrade, at the block at which the new federation,
/// made at block 1, is active and the old one retiring, a payment of a
/// satoshi over the initial cap to each one's Flyover address, refunded to
/// the LP. The refund, and the active and the retiring federation's Flyover
/// redeem scripts.
fn over_the_cap_to_both(
    active: (&[[u8; 33]], u64),
    retiring: (&[[u8; 33]], u64),
) -> (bitcoin::Transaction, Vec<u8>, Vec<u8>) {
    let block = 1 + Net::Mainnet.constants().federation_activation_age;
    let mut flyover = Flyover::new(Net::Mainnet, Vetiver900, block);
    flyover.store_as(NEW, active.0, active.1);
    flyover.store_as(OLD, retiring.0, retiring.1);
    let amount = flyover.bridge.constants.initial_locking_cap + 1;
    let tx = btc_tx(
        &[
            (amount, flyover.pays_to(active.0, active.1)),
            (amount, flyover.pays_to(retiring.0, retiring.1)),
        ],
        None,
    );
    let constants = &flyover.bridge.constants;
    let height = constants.btc_height_when_pegout_tx_index_activates
        + constants.pegout_tx_index_grace_period_in_btc_blocks;
    flyover.chain(height, txid(&tx));
    let raw = bitcoin::consensus::serialize(&tx);
    let answer = flyover.register(&raw, i64::from(height), &one_leaf_tree(&txid(&tx)), true);
    assert_eq!(int_of(&answer), REFUNDED_LP);
    let mut refund = flyover.waiting_for_confirmations();
    assert_eq!(refund.len(), 1);
    (
        refund.remove(0),
        flyover.redeem_of(active.0, active.1),
        flyover.redeem_of(retiring.0, retiring.1),
    )
}

/// What the last element of `input`'s script pushes, where rskj's
/// `assertScriptSigHasExpectedInputRedeemData` finds the redeem script.
fn last_push_of_script_sig(input: &bitcoin::TxIn) -> Vec<u8> {
    match input.script_sig.instructions().last() {
        Some(Ok(bitcoin::script::Instruction::PushBytes(bytes))) => bytes.as_bytes().to_vec(),
        other => panic!("the script ends with {other:?}"),
    }
}

/// rskj's `registerFlyoverBtcTransaction_fundsThatSurpassLockingCapSentToP2shErpActiveAndP2shErpRetiringFeds_shouldSetRedeemDataInInputsScriptSig`:
/// with both federations P2SH-ERP, the refund spends what each was paid, the
/// active federation's first, each input's script ending with its own
/// federation's Flyover redeem script.
#[test]
fn a_refund_from_two_p2sh_erp_federations_spends_each_with_its_own_flyover_script() {
    let (refund, active, retiring) = over_the_cap_to_both(
        (&seeded_keys("newMember", 9), P2SH_ERP),
        (&seeded_keys("member", 9), P2SH_ERP),
    );
    assert_eq!(refund.input.len(), 2);
    assert_eq!(last_push_of_script_sig(&refund.input[0]), active);
    assert_eq!(last_push_of_script_sig(&refund.input[1]), retiring);
}

/// rskj's `registerFlyoverBtcTransaction_fundsThatSurpassLockingCapSentToP2shP2wshErpActiveAndP2shErpRetiringFeds_shouldSetFixedScriptSig_shouldSetRedeemDataInInputsWitness`:
/// the refund is built in the active federation's format, so with a
/// P2SH-P2WSH one both inputs, the retiring P2SH-ERP federation's too, carry
/// their Flyover redeem script last in the witness and, as their script, a
/// push of its P2WSH program alone.
#[test]
fn a_refund_built_for_a_p2sh_p2wsh_federation_carries_both_redeem_scripts_in_witnesses() {
    use sha2::Digest;
    let (refund, active, retiring) = over_the_cap_to_both(
        (&seeded_keys("member", 20), P2SH_P2WSH_ERP),
        (&seeded_keys("member", 9), P2SH_ERP),
    );
    assert_eq!(refund.input.len(), 2);
    for (input, redeem) in refund.input.iter().zip([active, retiring]) {
        assert_eq!(input.witness.last(), Some(&redeem[..]));
        let program = [&[0x00, 0x20][..], &sha2::Sha256::digest(&redeem)].concat();
        let script = [&[program.len() as u8][..], &program].concat();
        assert_eq!(input.script_sig.as_bytes(), &script[..]);
    }
}
