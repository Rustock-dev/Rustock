//! rskj `co.rsk.peg.BridgeUtilsTest`: a Bitcoin address written and read with
//! its version (RSKIP284), the size of a peg-out (RSKIP271, RSKIP305), and the
//! value a Flyover peg-in sends to its federations (RSKIP293).
//!
//! rskj reaches these utilities from three Bridge methods, and so do these
//! tests: `registerFastBridgeBtcTransaction` reads and writes the addresses
//! and counts the value, `getEstimatedFeesForNextPegOutEvent` sizes a peg-out
//! of two inputs, and `releaseBtc` sizes the regular one for its minimum.

use alloy_primitives::{hex, Address as RskAddress, U256};

use super::support::{
    self, flyover_code, FlyoverCall, NEW_FEDERATION, OLD_FEDERATION, PEGIN_HEIGHT, WEI_PER_SATOSHI,
};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{peg, storage};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::BridgeTxContext;

// --- the address with its version (RSKIP284) --------------------------------

/// The hash160 of rskj's test addresses.
const HASH: [u8; 20] = hex!("41aec8ca3fcf17e62077e9f35961385360d6a570");

/// `[version || hash160]`.
fn address(version: u8) -> Vec<u8> {
    [&[version][..], &HASH[..]].concat()
}

/// A network, and the first block it has RSKIP284 at: testnet moves it to
/// #2,581,800, mainnet has it with hop400. The Flyover method opens with
/// iris300, before either.
struct Net {
    config: BridgeConstants,
    cfg: RskHardforkConfig,
    rskip284: u64,
}

fn testnet() -> Net {
    Net {
        config: BridgeConstants::testnet(),
        cfg: RskHardforkConfig::testnet(),
        rskip284: 2_581_800,
    }
}

fn mainnet() -> Net {
    Net {
        config: BridgeConstants::mainnet(),
        cfg: RskHardforkConfig::mainnet(),
        rskip284: 4_598_500,
    }
}

/// What a crafted call needs to reach the addresses: a version and an input
/// count of one.
const ONE_INPUT: [u8; 5] = [1, 0, 0, 0, 1];

/// A Flyover call made with `user` and `lp` as the two Bitcoin addresses.
fn call_with<'a>(user: &'a [u8], lp: &'a [u8]) -> FlyoverCall<'a> {
    FlyoverCall {
        tx: &ONE_INPUT,
        height: i64::from(i32::MAX),
        derivation_arguments: [0x11; 32],
        user,
        lbc: RskAddress::repeat_byte(0x42),
        lp,
    }
}

/// What the Flyover method answers at `block` when the derivation hash taken
/// over rskj's bytes for both addresses, `rskj_user` and `rskj_lp`, is marked
/// used: ALREADY_PROCESSED where rustock read and wrote them as rskj does.
fn answer_with_rskjs_bytes_used(
    net: &Net,
    block: u64,
    (user, lp): (&[u8], &[u8]),
    (rskj_user, rskj_lp): (&[u8], &[u8]),
) -> i128 {
    let call = call_with(user, lp);
    let mut ctx = support::ctx_at(block);
    support::mark_flyover_hash_used(&mut ctx, call.tx, &call.derivation_hash(rskj_user, rskj_lp));
    call.answer(&mut ctx, &net.config, &net.cfg)
}

/// rskj `BridgeUtils.serializeBtcAddressWithVersion` writes an address as its
/// version and its hash160: before RSKIP284 the version as
/// `BigInteger.valueOf(version).toByteArray()`, from it as one byte. Each
/// version an address can have there is written as one byte either way (a
/// testnet P2SH address, version 196, is refused before RSKIP284, so its
/// two-byte form is never written), and the Flyover derivation hash is taken
/// over it: a payment marked used under the hash of rskj's bytes is found
/// used, and one marked under nothing is not.
#[test]
fn a_flyover_address_is_written_as_its_version_byte_and_hash() {
    let (testnet, mainnet) = (testnet(), mainnet());
    for (case, net, block, version) in [
        (
            "p2pkh testnet before rskip284",
            &testnet,
            testnet.rskip284 - 1,
            0x6f,
        ),
        (
            "p2pkh mainnet before rskip284",
            &mainnet,
            mainnet.rskip284 - 1,
            0x00,
        ),
        (
            "p2sh mainnet before rskip284",
            &mainnet,
            mainnet.rskip284 - 1,
            0x05,
        ),
        (
            "p2pkh testnet after rskip284",
            &testnet,
            testnet.rskip284,
            0x6f,
        ),
        (
            "p2sh testnet after rskip284",
            &testnet,
            testnet.rskip284,
            0xc4,
        ),
        (
            "p2pkh mainnet after rskip284",
            &mainnet,
            mainnet.rskip284,
            0x00,
        ),
        (
            "p2sh mainnet after rskip284",
            &mainnet,
            mainnet.rskip284,
            0x05,
        ),
    ] {
        let rskj_bytes = [&[version][..], &HASH[..]].concat();
        let given = address(version);
        assert_eq!(
            answer_with_rskjs_bytes_used(net, block, (&given, &given), (&rskj_bytes, &rskj_bytes)),
            flyover_code::ALREADY_PROCESSED,
            "{case}"
        );
        let call = call_with(&given, &given);
        assert_ne!(
            call.answer(&mut support::ctx_at(block), &net.config, &net.cfg),
            flyover_code::ALREADY_PROCESSED,
            "{case}, nothing marked"
        );
    }
}

/// rskj `BridgeUtils.deserializeBtcAddressWithVersion` from RSKIP284: exactly
/// 21 bytes, the first an unsigned version the network takes, and the
/// address is that version and the 20 bytes after it. Where it throws,
/// `Bridge.registerFlyoverBtcTransaction` answers GENERIC_ERROR.
#[test]
fn from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes() {
    let (testnet, mainnet) = (testnet(), mainnet());
    for (case, net, version) in [
        ("p2pkh testnet", &testnet, 0x6f),
        ("p2sh testnet", &testnet, 0xc4),
        ("p2pkh mainnet", &mainnet, 0x00),
        ("p2sh mainnet", &mainnet, 0x05),
    ] {
        let given = address(version);
        assert_eq!(
            answer_with_rskjs_bytes_used(net, net.rskip284, (&given, &given), (&given, &given)),
            flyover_code::ALREADY_PROCESSED,
            "{case} is read as given"
        );
    }

    let with_extra_bytes = [address(0x6f), hex!("0000aaaaeeee1111ffff").to_vec()].concat();
    for (case, net, user) in [
        (
            "a testnet p2pkh address on mainnet",
            &mainnet,
            address(0x6f),
        ),
        ("a testnet p2sh address on mainnet", &mainnet, address(0xc4)),
        ("a mainnet p2sh address on testnet", &testnet, address(0x05)),
        ("bytes after the hash", &testnet, with_extra_bytes),
        ("a hash of one byte", &testnet, vec![0x6f, 0x41]),
        ("no bytes", &testnet, vec![]),
    ] {
        let lp = address(if std::ptr::eq(net, &mainnet) {
            0x00
        } else {
            0x6f
        });
        let call = call_with(&user, &lp);
        assert_eq!(
            call.answer(&mut support::ctx_at(net.rskip284), &net.config, &net.cfg),
            flyover_code::GENERIC_ERROR,
            "{case}"
        );
    }
}

// --- the size of a peg-out (RSKIP271, RSKIP305) ------------------------------

/// rskj `BridgeUtils.calculateLegacyTxSize` and `calculateSegwitTxSize`
/// (BridgeUtils.java:658-704), for a federation whose redeem script is
/// `redeem` bytes long and takes `signatures` signatures. A legacy spend is
/// what bitcoinj serializes for `inputs` inputs carrying the redeem script and
/// `outputs` outputs to the federation's P2SH address, and 72 bytes for each
/// signature of each input. A segwit spend is the BIP141 virtual size of an
/// input-less transaction with those outputs, 36 bytes of script for each
/// input, and the redeem script and the signatures as its witness.
fn rskj_pegout_size(
    redeem: usize,
    signatures: usize,
    inputs: usize,
    outputs: usize,
    segwit: bool,
) -> usize {
    let var_int = |n: usize| if n < 0xfd { 1 } else { 3 };
    // value, script length, OP_HASH160 <20 bytes> OP_EQUAL.
    let p2sh_output = 8 + 1 + 23;
    let signing = signatures * inputs * 72;
    if !segwit {
        // outpoint, script length, script, sequence.
        let input = 36 + var_int(redeem) + redeem + 4;
        return 4
            + var_int(inputs)
            + inputs * input
            + var_int(outputs)
            + outputs * p2sh_output
            + 4
            + signing;
    }
    let base = 4 + var_int(0) + var_int(outputs) + outputs * p2sh_output + 4 + inputs * 36;
    let total = base + signing + inputs * redeem;
    (total + 3 * base) / 4
}

/// What rustock's `getEstimatedFeesForNextPegOutEvent` answers on mainnet at
/// block 6,000,000, between RSKIP385 and RSKIP305, for `keys` stored as the
/// active federation in `format`, with a peg-out queue that makes `outputs`
/// outputs. There rskj sizes a peg-out of two inputs and two outputs more
/// than the queue has (`getEstimatedFeesFromInputsAndOutputsCount`,
/// BridgeSupport.java:2647), and at a fee of 1,000 satoshis per KB the fee is
/// the size.
fn estimated_pegout_size(
    config: &BridgeConstants,
    keys: &[[u8; 33]],
    format: u64,
    outputs: usize,
) -> usize {
    let cfg = RskHardforkConfig::mainnet();
    let mut ctx = support::ctx_at(6_000_000);
    support::store_federation(&mut ctx, NEW_FEDERATION, keys, 1, format);
    storage::bridge_store_u256(&mut ctx, storage::FEE_PER_KB_KEY, U256::from(1_000));
    let queued: Vec<peg::ReleaseRequest> = (2..outputs)
        .map(|_| peg::ReleaseRequest {
            btc_dest_hash160: [0x01; 20],
            amount_satoshis: 1_000_000,
            rsk_tx_hash: None,
        })
        .collect();
    storage::bridge_store_bytes(
        &mut ctx,
        storage::bridge_storage_key(storage::RELEASE_REQUEST_QUEUE_KEY),
        &peg::serialize_release_queue_legacy(&queued),
    );
    let out =
        peg::get_estimated_fees_for_next_pegout(&mut ctx, 0, config, &cfg).expect("an estimate");
    U256::from_be_slice(&out.bytes).to::<usize>()
}

/// `count` hex keys, from the private keys given, for a network's ERP keys.
fn erp_keys(privates: &[u16]) -> &'static [&'static str] {
    let hexes: Vec<&'static str> = privates
        .iter()
        .map(|private| {
            let key = hex::encode(support::public_key(&private.to_be_bytes()));
            &*Box::leak(key.into_boxed_str())
        })
        .collect();
    Box::leak(hexes.into_boxed_slice())
}

/// rskj's peg-out sizes, each for its test's federation and its shape: a
/// standard federation of 13 members (7 signatures), the P2SH-P2WSH ERP
/// federation of the same members (mainnet's four emergency keys and
/// 52,560-block delay, `P2shP2wshErpFederationBuilder`), a non-standard ERP
/// federation of the keys fa01 to fa03 with the emergency keys fa03 to fa05
/// and a 500-block delay, and the P2SH-P2WSH ERP federation of fa01 to fa03.
///
/// rskj calls the sizer with two inputs and one or two outputs more than the
/// peg-out queue holds (BridgeSupport.java:959, :1228, :2647); its tests also
/// size spends of 9 to 100 inputs, which no caller makes. For each federation
/// rustock's estimate is held to the sizer at two inputs and each of the
/// test's output counts, and the sizer to every number rskj's tests assert.
#[test]
fn pegout_sizes_are_rskjs_for_each_federation_format() {
    let mainnet = BridgeConstants::mainnet();
    let non_standard = BridgeConstants {
        erp_fed_pubkeys: erp_keys(&[0xfa03, 0xfa04, 0xfa05]),
        erp_fed_activation_delay: 500,
        ..BridgeConstants::mainnet()
    };
    let thirteen = support::keys(1, 13);
    let three: Vec<[u8; 33]> = [0xfa01u16, 0xfa02, 0xfa03]
        .iter()
        .map(|k| support::public_key(&k.to_be_bytes()))
        .collect();
    // (federation, its keys, format, segwit, rskj's (inputs, outputs, size)).
    type Sizes = &'static [(usize, usize, usize)];
    let federations: [(&str, &BridgeConstants, &[[u8; 33]], u64, bool, Sizes); 4] = [
        (
            "standard, 13 members",
            &mainnet,
            &thirteen,
            1000,
            false,
            &[
                (2, 2, 2058),
                (9, 2, 9002),
                (10, 20, 10570),
                (50, 200, 56010),
            ],
        ),
        (
            "p2sh-p2wsh erp, 13 members",
            &mainnet,
            &thirteen,
            4000,
            true,
            &[(2, 2, 694), (9, 2, 2866), (10, 20, 3752), (50, 200, 21922)],
        ),
        (
            "non-standard erp, 3 members",
            &non_standard,
            &three,
            2000,
            false,
            &[(50, 200, 26510), (100, 50, 41810)],
        ),
        (
            "p2sh-p2wsh erp, 3 members",
            &mainnet,
            &three,
            4000,
            true,
            &[(50, 200, 13172), (100, 50, 15135)],
        ),
    ];
    let cfg = RskHardforkConfig::mainnet();
    for (federation, config, keys, format, segwit, sizes) in federations {
        let redeem = peg::federation_redeem_for_format_pub(keys, format, config, &cfg, 6_000_000);
        let signatures = keys.len() / 2 + 1;
        for &(inputs, outputs, size) in sizes {
            assert_eq!(
                rskj_pegout_size(redeem.len(), signatures, inputs, outputs, segwit),
                size,
                "{federation}: {inputs} inputs, {outputs} outputs"
            );
            assert_eq!(
                estimated_pegout_size(config, keys, format, outputs),
                rskj_pegout_size(redeem.len(), signatures, 2, outputs, segwit),
                "{federation}: 2 inputs, {outputs} outputs"
            );
        }
    }
}

/// rskj `BridgeUtils.getRegularPegoutTxSize`: a peg-out of two inputs and two
/// outputs, within 10% of a real one, signed by two of three members. It is
/// what `releaseBtc` asks a peg-out to be worth from RSKIP219: the fee per KB
/// times that size, plus the network's percentage on top
/// (`BridgeSupport.requestRelease`, BridgeSupport.java:958-983). A request
/// worth that much is queued, and one a satoshi short is not.
#[test]
fn the_regular_pegout_size_sets_releasebtcs_minimum() {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let keys = support::keys(1, 3);
    let redeem = support::standard_redeem(&keys);
    let size = rskj_pegout_size(redeem.len(), 2, 2, 2, false);
    assert_eq!(size, 654);

    // A real one: each input's script an OP_0, two 72-byte signatures and the
    // redeem script, which takes OP_PUSHDATA1; two P2PKH outputs.
    let signed_input = 1 + 2 * (1 + 72) + 2 + redeem.len();
    let real = 4 + 1 + 2 * (36 + 3 + signed_input + 4) + 1 + 2 * (8 + 1 + 25) + 4;
    assert!(real.abs_diff(size) * 10 < size, "{real} against {size}");

    let fee_per_kb = 1_000_000u64;
    let fee = fee_per_kb * size as u64 / 1000;
    let minimum = fee + fee * config.minimum_pegout_value_percentage_to_receive_after_fee / 100;
    assert!(
        minimum > config.minimum_pegout_tx_value,
        "the fee is what binds"
    );
    let queued =
        |satoshis| support::release_queued((&config, &cfg), 6_000_000, &keys, fee_per_kb, satoshis);
    assert_eq!(queued(minimum), 1);
    assert_eq!(queued(minimum - 1), 0);
}

// --- the value a Flyover peg-in sends (RSKIP293) -----------------------------

/// The RSK block the payments are registered at: past the regtest federation
/// activation age of the new federation, created at block 1, either side of
/// RSKIP383.
const RSK_BLOCK: u64 = 1_000;

/// The user's and the liquidity provider's address: regtest P2PKH.
const USER: [u8; 21] = {
    let mut address = [0x51; 21];
    address[0] = 0x6f;
    address
};

/// The Flyover call that registers `raw`, at the height the chain holds it.
fn call(raw: &[u8]) -> FlyoverCall<'_> {
    FlyoverCall {
        tx: raw,
        height: PEGIN_HEIGHT,
        derivation_arguments: [0x11; 32],
        user: &USER,
        lbc: RskAddress::repeat_byte(0x42),
        lp: &USER,
    }
}

/// The active federation's members, and the retiring one's.
fn active() -> Vec<[u8; 33]> {
    support::keys(1, 3)
}

fn retiring() -> Vec<[u8; 33]> {
    support::keys(11, 3)
}

/// The address a federation of `keys` takes this Flyover payment at.
fn flyover_address(keys: &[[u8; 33]]) -> bitcoin::ScriptBuf {
    let hash = call(&[]).derivation_hash(&USER, &USER);
    support::flyover_output(&hash, &support::standard_redeem(keys))
}

/// RSKj's regtest before RSKIP293: hop400 and what follows it past the block
/// the payments are registered at.
fn before_rskip293() -> RskHardforkConfig {
    support::upgrades_from_at(
        RskHardforkConfig::rskj_regtest(),
        RskNetworkUpgrade::Hop400,
        1_000_000,
    )
}

/// What a Flyover payment `tx` is answered with, its Bitcoin chain and the
/// Bridge's funds in place, the active federation `active()`, and with
/// `with_retiring` the retiring one `retiring()`; and the Bridge after it.
fn pay(
    tx: &bitcoin::Transaction,
    cfg: &RskHardforkConfig,
    with_retiring: bool,
) -> (i128, impl crate::RskContextTr) {
    let raw = bitcoin::consensus::serialize(tx);
    let mut ctx = support::ctx_at(RSK_BLOCK);
    support::btc_chain_holding(&mut ctx, &raw, cfg);
    support::store_federation(&mut ctx, NEW_FEDERATION, &active(), 1, 1000);
    if with_retiring {
        support::store_federation(&mut ctx, OLD_FEDERATION, &retiring(), 1, 1000);
    }
    support::fund_bridge(&mut ctx);
    let answer = call(&raw).answer(&mut ctx, &BridgeConstants::rskj_regtest(), cfg);
    (answer, ctx)
}

/// The UTXOs, by output index and value, the active federation holds.
fn active_utxos<CTX: crate::RskContextTr>(
    ctx: &mut CTX,
    cfg: &RskHardforkConfig,
) -> Vec<(u32, u64)> {
    storage::load_federation_utxos(ctx, cfg, RSK_BLOCK)
        .iter()
        .map(|utxo| (utxo.vout, utxo.value_satoshis))
        .collect()
}

/// The same for the retiring federation.
fn retiring_utxos<CTX: crate::RskContextTr>(ctx: &mut CTX) -> Vec<(u32, u64)> {
    storage::load_old_federation_utxos(ctx)
        .iter()
        .map(|utxo| (utxo.vout, utxo.value_satoshis))
        .collect()
}

/// The answer of a peg-in of `satoshis`: their value in wei.
fn locked(satoshis: u64) -> i128 {
    i128::from(satoshis) * i128::from(WEI_PER_SATOSHI)
}

const BTC: u64 = 100_000_000;

/// rskj `BridgeUtils.validateFlyoverPeginValue` turns a payment of nothing to
/// the federation down with UNPROCESSABLE_TX_VALUE_ZERO_ERROR, before and from
/// RSKIP293: an output of 0 to it, or no output to it at all, whatever the
/// payment sends elsewhere. Nothing is registered.
#[test]
fn a_flyover_payment_of_nothing_to_the_federation_is_refused() {
    let others = |n: u8| support::p2pkh(&[n; 20]);
    for cfg in [before_rskip293(), RskHardforkConfig::rskj_regtest()] {
        let rskip293 = cfg.has_rskip293(RSK_BLOCK);
        for tx in [
            support::tx_paying(&[
                (0, flyover_address(&active())),
                (BTC, others(1)),
                (BTC, others(2)),
            ]),
            support::tx_paying(&[(BTC, others(1)), (BTC, others(2))]),
        ] {
            let (answer, mut ctx) = pay(&tx, &cfg, false);
            assert_eq!(answer, flyover_code::VALUE_ZERO, "rskip293 {rskip293}");
            assert!(active_utxos(&mut ctx, &cfg).is_empty());
        }
    }
}

/// Before RSKIP293 a Flyover payment is checked against no minimum: a UTXO
/// below the minimum peg-in is taken, and every output to the federation's
/// address is registered, a 0-value one too (`BridgeUtilsLegacy
/// .getUTXOsSentToAddress`).
#[test]
fn before_rskip293_every_flyover_output_is_taken_whatever_its_value() {
    let cfg = before_rskip293();
    let minimum = BridgeConstants::rskj_regtest().minimum_pegin_tx_value;
    let elsewhere = support::p2pkh(&[0x01; 20]);

    let tx = support::tx_paying(&[
        (minimum - 1, flyover_address(&active())),
        (BTC, elsewhere.clone()),
    ]);
    let (answer, mut ctx) = pay(&tx, &cfg, false);
    assert_eq!(answer, locked(minimum - 1));
    assert_eq!(active_utxos(&mut ctx, &cfg), [(0, minimum - 1)]);

    let tx = support::tx_paying(&[
        (BTC, flyover_address(&active())),
        (0, flyover_address(&active())),
        (BTC, elsewhere),
    ]);
    let (answer, mut ctx) = pay(&tx, &cfg, false);
    assert_eq!(answer, locked(BTC));
    assert_eq!(active_utxos(&mut ctx, &cfg), [(0, BTC), (1, 0)]);
}

/// From RSKIP293 each UTXO a Flyover payment sends its federations must be
/// worth the minimum peg-in (`PegUtils.allUTXOsToFedAreAboveMinimumPeginValue`):
/// one a satoshi short of it is answered
/// UNPROCESSABLE_TX_UTXO_AMOUNT_SENT_BELOW_MINIMUM_ERROR, whatever the others
/// are worth, and UTXOs of the minimum and more are taken.
#[test]
fn from_rskip293_each_flyover_utxo_must_be_worth_the_minimum() {
    let cfg = RskHardforkConfig::rskj_regtest();
    let minimum = BridgeConstants::rskj_regtest().minimum_pegin_tx_value;

    let tx = support::tx_paying(&[
        (minimum - 1, flyover_address(&active())),
        (BTC, flyover_address(&active())),
    ]);
    let (answer, mut ctx) = pay(&tx, &cfg, false);
    assert_eq!(answer, flyover_code::UTXO_BELOW_MINIMUM);
    assert!(active_utxos(&mut ctx, &cfg).is_empty());

    for second in [minimum, minimum + 1] {
        let tx = support::tx_paying(&[
            (minimum, flyover_address(&active())),
            (second, flyover_address(&retiring())),
        ]);
        let (answer, _) = pay(&tx, &cfg, true);
        assert_eq!(answer, locked(minimum + second));
    }
}

/// rskj `BridgeUtils.getAmountSentToAddresses` from RSKIP293: what a Flyover
/// payment sends to the active federation's address and the retiring one's,
/// together.
#[test]
fn from_rskip293_a_flyover_payment_counts_what_both_federations_are_sent() {
    let cfg = RskHardforkConfig::rskj_regtest();
    for (case, outputs, expected) in [
        (
            "both",
            vec![
                (BTC, flyover_address(&active())),
                (BTC, flyover_address(&retiring())),
            ],
            2 * BTC,
        ),
        (
            "the active one",
            vec![(BTC, flyover_address(&active()))],
            BTC,
        ),
        (
            "the retiring one",
            vec![(BTC, flyover_address(&retiring()))],
            BTC,
        ),
    ] {
        let (answer, _) = pay(&support::tx_paying(&outputs), &cfg, true);
        assert_eq!(answer, locked(expected), "{case}");
    }
}

/// rskj `BridgeUtils.getUTXOsSentToAddresses` from RSKIP293, called for each
/// federation's address (BridgeSupport.java:2851, :2871): each federation
/// keeps the UTXOs sent to it, by their index in the payment, and an output
/// to anyone else, a native segwit one too, is passed over.
#[test]
fn from_rskip293_each_federation_keeps_the_flyover_utxos_it_is_sent() {
    let cfg = RskHardforkConfig::rskj_regtest();
    let tx = support::tx_paying(&[
        (BTC, flyover_address(&active())),
        (2 * BTC, flyover_address(&retiring())),
        (BTC, support::p2pkh(&[0x01; 20])),
        (3 * BTC, flyover_address(&active())),
    ]);
    let (answer, mut ctx) = pay(&tx, &cfg, true);
    assert_eq!(answer, locked(6 * BTC));
    assert_eq!(active_utxos(&mut ctx, &cfg), [(0, BTC), (3, 3 * BTC)]);
    assert_eq!(retiring_utxos(&mut ctx), [(1, 2 * BTC)]);

    let tx = support::tx_paying(&[
        (BTC, support::p2wpkh(&[0x01; 20])),
        (BTC, flyover_address(&active())),
    ]);
    let (answer, mut ctx) = pay(&tx, &cfg, false);
    assert_eq!(answer, locked(BTC));
    assert_eq!(active_utxos(&mut ctx, &cfg), [(1, BTC)]);
}

/// Before RSKIP293 rskj counts a Flyover payment by reading every one of its
/// outputs as an address (`BridgeUtilsLegacy.getAmountSentToAddress`,
/// BridgeUtilsLegacy.java:104-108). bitcoinj's `Script.getToAddress` throws
/// for an output that is neither P2PKH nor P2SH (Script.java:320-329), a
/// native segwit one or an OP_RETURN, and `Bridge.registerFlyoverBtcTransaction`
/// answers the throw with GENERIC_ERROR (Bridge.java:1382-1385): nothing is
/// transferred or registered.
#[test]
fn before_rskip293_a_flyover_payment_with_an_output_that_is_no_address_is_a_generic_error() {
    let cfg = before_rskip293();
    let op_return = bitcoin::ScriptBuf::from_bytes(vec![0x6a, 4, 1, 2, 3, 4]);
    for no_address in [support::p2wpkh(&[0x01; 20]), op_return] {
        let tx = support::tx_paying(&[(BTC, no_address), (BTC, flyover_address(&active()))]);
        let (answer, mut ctx) = pay(&tx, &cfg, false);
        assert_eq!(answer, flyover_code::GENERIC_ERROR);
        assert!(active_utxos(&mut ctx, &cfg).is_empty());
    }
}

// --- the signatures of a peg-out ---------------------------------------------

/// rskj `BridgeUtils.hasEnoughSignatures`, which `addSignature` asks before it
/// releases a peg-out (BridgeSupport.java:1741): each input has a signature
/// in every place its script left one for, the OP_0s the transaction was
/// built with all replaced. One missing on any input, or all of them, and
/// the peg-out is not released.
#[test]
fn a_pegout_missing_a_signature_on_any_input_has_not_enough_signatures() {
    let redeem = support::standard_redeem(&support::keys(1, 3));
    // `ScriptBuilder.createMultiSigInputScriptBytes`: OP_0, each signature,
    // an empty one as OP_0, then the redeem script, which takes OP_PUSHDATA1.
    let input_script = |signatures: [&[u8]; 2]| {
        let mut script = vec![0x00];
        for signature in signatures {
            script.push(signature.len() as u8);
            script.extend_from_slice(signature);
        }
        script.extend_from_slice(&[0x4c, redeem.len() as u8]);
        script.extend_from_slice(&redeem);
        bitcoin::ScriptBuf::from_bytes(script)
    };
    let pegout = |inputs: usize, signatures: [&[u8]; 2]| {
        let mut tx = support::tx_paying(&[(BTC, support::p2pkh(&[0x01; 20]))]);
        tx.input = vec![tx.input[0].clone(); inputs];
        for input in &mut tx.input {
            input.script_sig = input_script(signatures);
        }
        crate::bridge::release_tx::has_enough_signatures(&tx)
    };
    let (one, other, missing): (&[u8], &[u8], &[u8]) = (&[0x79], &[0x78], &[]);
    assert!(pegout(1, [one, other]));
    assert!(!pegout(1, [one, missing]));
    assert!(!pegout(1, [missing, missing]));
    assert!(pegout(3, [one, other]));
    assert!(!pegout(3, [one, missing]));
}

// --- a Coin from the ABI ------------------------------------------------------

/// rskj `BridgeUtils.getCoinFromBigInteger`, with which
/// `Bridge.increaseLockingCap` reads its argument (Bridge.java:1306): the
/// number's `longValueExact`, which throws for one past a Java long, and the
/// call fails. A number that fits is read, and the increase taken.
#[test]
fn a_locking_cap_past_a_java_long_fails_the_increase() {
    use crate::abi::encode::{self, Arg};
    let config = BridgeConstants::mainnet();
    let authorizer = hex::decode(config.increase_locking_cap_authorizer_keys[0]).expect("hex");
    let tx_ctx = BridgeTxContext {
        rsk_sender: crate::bridge::federation::rsk_address_from_public_key(&authorizer)
            .expect("a key"),
        ..Default::default()
    };
    let mut ctx = support::ctx_at(6_000_000);
    let mut increase = |cap: U256| {
        let args = encode::args(&[Arg::Word(cap.to_be_bytes::<32>())]);
        crate::bridge::governance::increase_locking_cap(&mut ctx, &args, 0, &config, &tx_ctx)
            .map(|out| out.bytes[31] == 1)
    };
    assert!(increase(U256::from(1u64 << 63)).is_err(), "one past a long");
    assert_eq!(
        increase(U256::from(config.initial_locking_cap + BTC)),
        Ok(true),
        "a cap that fits"
    );
}
