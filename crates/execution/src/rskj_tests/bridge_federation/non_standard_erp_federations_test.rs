//! rskj `co.rsk.peg.federation.NonStandardErpFederationsTest`, with
//! `NonStandardErpRedeemScriptBuilderFactoryTest` and `FederationFactoryTest`:
//! the redeem script of a non-standard ERP federation (format 2000), which
//! rskj builds anew with the activations of the block being run.

use super::support::{self, key};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::peg;
use crate::hardfork::RskHardforkConfig;

type Network = (BridgeConstants, RskHardforkConfig);

fn testnet() -> Network {
    (BridgeConstants::testnet(), RskHardforkConfig::testnet())
}

fn mainnet() -> Network {
    (BridgeConstants::mainnet(), RskHardforkConfig::mainnet())
}

/// Testnet's last block before RSKIP284, its first with it, and its first
/// with RSKIP293; mainnet's first block with RSKIP201 (iris300) and its first
/// with RSKIP284 and RSKIP293 (hop400).
const TESTNET_BEFORE_RSKIP284: u64 = 2_581_799;
const TESTNET_RSKIP284: u64 = 2_581_800;
const TESTNET_RSKIP293: u64 = 3_103_000;
const MAINNET_RSKIP201: u64 = 3_614_800;
const MAINNET_RSKIP293: u64 = 4_598_500;

/// 52,560, the delay of mainnet and testnet, as each builder pushes it:
/// before RSKIP293 two unsigned big-endian bytes (bitcoinj's
/// `unsignedLongToByteArrayBE(delay, 2)`), from it Java's
/// `BigInteger.toByteArray` reversed (`signedLongToByteArrayLE`).
const DELAY_BEFORE_RSKIP293: &[u8] = &[0xcd, 0x50];
const DELAY_FROM_RSKIP293: &[u8] = &[0x50, 0xcd, 0x00];

/// The nine members of the federation rskj's tests build.
fn members() -> Vec<[u8; 33]> {
    [
        "03b53899c390573471ba30e5054f78376c5f797fda26dde7a760789f02908cbad2",
        "027319afb15481dbeb3c426bcc37f9a30e7f51ceff586936d85548d9395bcc2344",
        "0355a2e9bf100c00fc0a214afd1bf272647c7824eb9cb055480962f0c382596a70",
        "02566d5ded7c7db1aa7ee4ef6f76989fb42527fcfdcddcd447d6793b7d869e46f7",
        "0294c817150f78607566e961b3c71df53a22022a80acbb982f83c0c8baac040adc",
        "0372cd46831f3b6afd4c044d160b7667e8ebf659d6cb51a825a3104df6ee0638c6",
        "0340df69f28d69eef60845da7d81ff60a9060d4da35c767f017b0dd4e20448fb44",
        "02ac1901b6fba2c1dbd47d894d2bd76c8ba1d296d65f6ab47f1c6b22afb53e73eb",
        "031aabbeb9b27258f98c2bf21f36677ae7bae09eb2d8c958ef41a20a6e88626d26",
    ]
    .map(key)
    .to_vec()
}

/// The redeem script rustock gives a non-standard ERP federation of
/// `members` at `block` of `network`.
fn redeem(members: &[[u8; 33]], network: &Network, block: u64) -> Vec<u8> {
    peg::federation_redeem_for_format_pub(members, 2000, &network.0, &network.1, block)
}

/// rskj's `getRedeemScript_*` and `createErpFederation_*` tests: from RSKIP201
/// the script is the members' multisig and the network's emergency multisig,
/// laid out as rskj lays it out, with the delay in two big-endian bytes before
/// RSKIP293 and as a Java number from it. Before RSKIP284 testnet's
/// federation has one script, whatever its members (`FederationFactoryTest`):
/// so the same members have three scripts on testnet, and two on mainnet,
/// where the script before RSKIP284 is the one after it.
#[test]
fn a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era() {
    let members = members();
    for (era, network, block, delay) in [
        (
            "mainnet from RSKIP201",
            mainnet(),
            MAINNET_RSKIP201,
            DELAY_BEFORE_RSKIP293,
        ),
        (
            "mainnet before RSKIP293",
            mainnet(),
            MAINNET_RSKIP293 - 1,
            DELAY_BEFORE_RSKIP293,
        ),
        (
            "mainnet from RSKIP293",
            mainnet(),
            MAINNET_RSKIP293,
            DELAY_FROM_RSKIP293,
        ),
        (
            "testnet from RSKIP284",
            testnet(),
            TESTNET_RSKIP284,
            DELAY_BEFORE_RSKIP293,
        ),
        (
            "testnet from RSKIP293",
            testnet(),
            TESTNET_RSKIP293,
            DELAY_FROM_RSKIP293,
        ),
    ] {
        let emergency = support::erp_keys_of(&network.0);
        assert_eq!(
            redeem(&members, &network, block),
            support::non_standard_erp_redeem(&members, &emergency, delay),
            "{era}"
        );
    }

    let hardcoded = redeem(&members, &testnet(), TESTNET_BEFORE_RSKIP284);
    assert_eq!(
        redeem(&support::keys(1, 5), &testnet(), TESTNET_BEFORE_RSKIP284),
        hardcoded
    );
    let unsigned_delay = redeem(&members, &testnet(), TESTNET_RSKIP284);
    let signed_delay = redeem(&members, &testnet(), TESTNET_RSKIP293);
    assert!(
        hardcoded != unsigned_delay && hardcoded != signed_delay && unsigned_delay != signed_delay
    );

    assert_eq!(
        redeem(&members, &mainnet(), MAINNET_RSKIP201),
        redeem(&members, &mainnet(), MAINNET_RSKIP293 - 1)
    );
    assert_ne!(
        redeem(&members, &mainnet(), MAINNET_RSKIP293 - 1),
        redeem(&members, &mainnet(), MAINNET_RSKIP293)
    );
}

/// rskj's `createFederation_postRSKIP293_withValidCsvValues_valid` (130, 500,
/// 33,000 and 65,535, the largest delay rskj takes) and
/// `createFederation_postRSKIP284_preRSKIP293_withValidCsvValueOneByteLong_valid`
/// (20, padded to two bytes on testnet before RSKIP293).
#[test]
fn a_delay_is_pushed_in_two_big_endian_bytes_before_rskip293_and_as_a_java_number_from_it() {
    let members = members();
    let emergency = support::erp_keys_of(&BridgeConstants::mainnet());
    for (delay, network, block, pushed) in [
        (20, testnet(), TESTNET_RSKIP284, &[0x00, 0x14][..]),
        (130, mainnet(), MAINNET_RSKIP293, &[0x82, 0x00][..]),
        (500, mainnet(), MAINNET_RSKIP293, &[0xf4, 0x01][..]),
        (33_000, mainnet(), MAINNET_RSKIP293, &[0xe8, 0x80, 0x00][..]),
        (65_535, mainnet(), MAINNET_RSKIP293, &[0xff, 0xff, 0x00][..]),
    ] {
        let network = (support::with_erp(network.0, &emergency, delay), network.1);
        assert_eq!(
            redeem(&members, &network, block),
            support::non_standard_erp_redeem(&members, &emergency, pushed),
            "a delay of {delay}"
        );
    }
}

/// rskj's `createdFederationInfo_withRealValues_equalsExistingFederationInfo
/// _testnet`: the last non-standard ERP federation testnet had, of five
/// members with testnet's emergency keys and delay, from RSKIP293.
#[test]
fn the_last_nonstandard_erp_federation_of_testnet_has_its_real_address() {
    let members = [
        "0208f40073a9e43b3e9103acec79767a6de9b0409749884e989960fee578012fce",
        "0225e892391625854128c5c4ea4340de0c2a70570f33db53426fc9c746597a03f4",
        "025a2f522aea776fab5241ad72f7f05918e8606676461cb6ce38265a52d4ca9ed6",
        "02afc230c2d355b1a577682b07bc2646041b5d0177af0f98395a46018da699b6da",
        "0344a3c38cd59afcba3edcebe143e025574594b001700dec41e59409bdbd0f2a09",
    ]
    .map(key);
    let redeem = redeem(&members, &testnet(), TESTNET_RSKIP293);
    assert_eq!(
        hex::encode(support::p2sh(&support::hash160(&redeem)).as_bytes()),
        "a91412d5d2996618c8abcb1e6fc17be3cd8e2790c25f87"
    );
    let address = bitcoin::Address::p2sh(
        bitcoin::Script::from_bytes(&redeem),
        bitcoin::Network::Testnet,
    )
    .expect("a redeem script within 520 bytes");
    assert_eq!(address.to_string(), "2MtxpJPt2xCa3AyFYUjTT7Aop9Z6gGf4rqA");
}

/// Four of the vectors in rskj's `redeemScripts.json`, written by another
/// implementation: the smallest script, one with a delay in two bytes and one
/// in three (each with both its key lists out of order), and the largest
/// within 520 bytes. (members, emergency keys, delay, script).
const OTHER_IMPLEMENTATION: [(&[&str], &[&str], i64, &str); 4] = [
    (
        &["0366f012edffa9160f0fa4cade9c8d58e1f092d99a9aa0500cfe54a0299fb0ed8c"],
        &["022eaf5c60b49ca7902296cf692e6a3533dadf47cca8a5fce95145dedd8492c66b"],
        7065,
        "6451210366f012edffa9160f0fa4cade9c8d58e1f092d99a9aa0500cfe54a0299fb0ed8c516702991bb2755121022eaf5c60b49ca7902296cf692e6a3533dadf47cca8a5fce95145dedd8492c66b5168ae",
    ),
    (
        &[
            "03e54b21e1b8e60d145f63ada38438be62030f1755695ce9b0486dc387784e2065",
            "0325ff8a972ab62d46a35ab8faa0cde7d325e9d838abbddfe222143c7b9b869e22",
            "03beb566feb61d8220ae4b63c91512386cb94047ad10d70106fe41232d81c28970",
            "0225917ca941b51891a2319ab36e1401bfe3a3bdf42cc3acb2d25922aebc99926d",
        ],
        &[
            "03e3191c756eb874a49a26a574772ab69be0961178fa6721019e81e3bb67ab1b55",
            "025e0059d88266f1b6100d5f631e21ea58c166212f5e311e33d8664560af44b396",
            "03be26cbfdd5139c5a6354972ad3f55e739832a7baf6083be1d26b0902cd8bc0e2",
        ],
        29405,
        "6453210225917ca941b51891a2319ab36e1401bfe3a3bdf42cc3acb2d25922aebc99926d210325ff8a972ab62d46a35ab8faa0cde7d325e9d838abbddfe222143c7b9b869e222103beb566feb61d8220ae4b63c91512386cb94047ad10d70106fe41232d81c289702103e54b21e1b8e60d145f63ada38438be62030f1755695ce9b0486dc387784e2065546702dd72b2755221025e0059d88266f1b6100d5f631e21ea58c166212f5e311e33d8664560af44b3962103be26cbfdd5139c5a6354972ad3f55e739832a7baf6083be1d26b0902cd8bc0e22103e3191c756eb874a49a26a574772ab69be0961178fa6721019e81e3bb67ab1b555368ae",
    ),
    (
        &[
            "02d24ce2e6771690763ea3277da12d73f9177ce0e45981a81e6c953fb9fdd3b1e3",
            "03c90c67fb3943b2208ef36ce0389e9b0ec4ac5bdc0bb4a77838cb7442c8edcc9d",
            "02272166c3f5f5a8b998dbe78675f8cec7cf77eefc1146d4c3c67796604d25495c",
        ],
        &[
            "02eb7bb04e139caf711b3eda305a39ba94b85d0460ad391624bc01901ea2a33f29",
            "02d741e20d65d229748f7cc4e0b4fe8d33095f9ecb2111876f16291a6914410a9b",
        ],
        48635,
        "64522102272166c3f5f5a8b998dbe78675f8cec7cf77eefc1146d4c3c67796604d25495c2102d24ce2e6771690763ea3277da12d73f9177ce0e45981a81e6c953fb9fdd3b1e32103c90c67fb3943b2208ef36ce0389e9b0ec4ac5bdc0bb4a77838cb7442c8edcc9d536703fbbd00b275522102d741e20d65d229748f7cc4e0b4fe8d33095f9ecb2111876f16291a6914410a9b2102eb7bb04e139caf711b3eda305a39ba94b85d0460ad391624bc01901ea2a33f295268ae",
    ),
    (
        &[
            "0261e78783cb9d3097abaccf9ac91eba0a6407075a7fccffe9c95273d863cec2ca",
            "022d63d0ee9148e194eb6b44f9bcf4dd6cc15428d4c65d879cd5ff656bca399c22",
            "02b65f56a85e04d478b0af7f71cde1a0b7615027ce563e22f0ab318c0f29278d7a",
            "02114ba2c7866000c85bfc47cf88ba0b395e7674d424dfa5a4388cf2a807679196",
            "03d6f2ec1a41c30bf5b6f751562ad7096fc3eeecd274ef588c1b7dc0c97164472f",
            "0363ecb2e65648242f4d4e0d5194c7f22a9ffee217e09a24d8c3e9780eae7b0df0",
            "0261347be3cc4e1a72a473215a1d921d272c07e12ac9872915f78c689a39e465e9",
            "02d3a2e74cdf240964c48e1a1e3d27a411d7b5f39eb1e565a744f96732a4015a72",
            "034907d2a94d603181dfc83106cec71f2e1c1f04b55b66c947379399560748bb8e",
            "0268766a465d446db3e093c8e5ae260c344718c26ff2c95c2bc64dccb1e61b0be9",
            "033846e3a41c46bbb301e74bc70d9e61b83ca93dd3d83302c6e121b1da6f71483f",
        ],
        &[
            "03e7c8d6300e410029c0a12d33c40754093f7d827d60c306125c0895c0d3304567",
            "020f829a7fd6bc4414024269a966ce4fa8427655fa58f72930f38e97e7d15d5073",
            "03d9ad29198e900cc2dcfc7661386daa4ad6fba733f310a2f4e46d512371decb2b",
        ],
        48979,
        "64562102114ba2c7866000c85bfc47cf88ba0b395e7674d424dfa5a4388cf2a80767919621022d63d0ee9148e194eb6b44f9bcf4dd6cc15428d4c65d879cd5ff656bca399c22210261347be3cc4e1a72a473215a1d921d272c07e12ac9872915f78c689a39e465e9210261e78783cb9d3097abaccf9ac91eba0a6407075a7fccffe9c95273d863cec2ca210268766a465d446db3e093c8e5ae260c344718c26ff2c95c2bc64dccb1e61b0be92102b65f56a85e04d478b0af7f71cde1a0b7615027ce563e22f0ab318c0f29278d7a2102d3a2e74cdf240964c48e1a1e3d27a411d7b5f39eb1e565a744f96732a4015a7221033846e3a41c46bbb301e74bc70d9e61b83ca93dd3d83302c6e121b1da6f71483f21034907d2a94d603181dfc83106cec71f2e1c1f04b55b66c947379399560748bb8e210363ecb2e65648242f4d4e0d5194c7f22a9ffee217e09a24d8c3e9780eae7b0df02103d6f2ec1a41c30bf5b6f751562ad7096fc3eeecd274ef588c1b7dc0c97164472f5b670353bf00b2755221020f829a7fd6bc4414024269a966ce4fa8427655fa58f72930f38e97e7d15d50732103d9ad29198e900cc2dcfc7661386daa4ad6fba733f310a2f4e46d512371decb2b2103e7c8d6300e410029c0a12d33c40754093f7d827d60c306125c0895c0d33045675368ae",
    ),
];

/// rskj's `getNonStandardErpRedeemScript_compareOtherImplementation`: on
/// testnet from RSKIP293, the members and emergency keys of each vector, with
/// its delay, give its script.
#[test]
fn nonstandard_erp_redeem_scripts_are_those_another_implementation_writes() {
    for (members, emergency, delay, script) in OTHER_IMPLEMENTATION {
        let members: Vec<[u8; 33]> = members.iter().map(|hex| key(hex)).collect();
        let emergency: Vec<[u8; 33]> = emergency.iter().map(|hex| key(hex)).collect();
        let network = (
            support::with_erp(BridgeConstants::testnet(), &emergency, delay),
            RskHardforkConfig::testnet(),
        );
        assert_eq!(
            hex::encode(redeem(&members, &network, TESTNET_RSKIP293)),
            script,
            "a delay of {delay}"
        );
    }
}

/// The delay Bitcoin's `OP_CHECKSEQUENCEVERIFY` reads from a non-standard ERP
/// script of `members` members: the push after `OP_ELSE`, read as a script
/// number, little-endian.
fn delay_bitcoin_reads(script: &[u8], members: usize) -> i64 {
    let at = 3 + 34 * members;
    assert_eq!(script[at], 0x67, "OP_ELSE");
    let length = usize::from(script[at + 1]);
    bitcoin::script::read_scriptint(&script[at + 2..at + 2 + length]).expect("a script number")
}

/// rskj's `spendFromNonStandardErpFed_*_using_erp_multisig_*`: the emergency
/// keys spend an input whose sequence is the network's delay, and
/// `OP_CHECKSEQUENCEVERIFY` passes when the delay the script holds is no more
/// than it. Before RSKIP293 the delay's two bytes are big-endian and Bitcoin
/// reads them little-endian: 52,560 is read as 20,685, so the emergency keys
/// spend early, and 300 as 11,265, so they cannot spend at 300. From RSKIP293
/// the script holds the delay itself.
#[test]
fn before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes() {
    let members = support::keys(1, 9);
    for (era, network, block, delay, read) in [
        (
            "testnet before RSKIP293",
            testnet(),
            TESTNET_RSKIP284,
            52_560,
            20_685,
        ),
        (
            "testnet before RSKIP293",
            testnet(),
            TESTNET_RSKIP284,
            300,
            11_265,
        ),
        (
            "mainnet before RSKIP293",
            mainnet(),
            MAINNET_RSKIP293 - 1,
            52_560,
            20_685,
        ),
        (
            "mainnet before RSKIP293",
            mainnet(),
            MAINNET_RSKIP293 - 1,
            300,
            11_265,
        ),
        (
            "testnet from RSKIP293",
            testnet(),
            TESTNET_RSKIP293,
            52_560,
            52_560,
        ),
        (
            "mainnet from RSKIP293",
            mainnet(),
            MAINNET_RSKIP293,
            52_560,
            52_560,
        ),
    ] {
        let emergency = support::erp_keys_of(&network.0);
        let network = (support::with_erp(network.0, &emergency, delay), network.1);
        let script = redeem(&members, &network, block);
        assert_eq!(
            delay_bitcoin_reads(&script, members.len()),
            read,
            "{era}, a delay of {delay}"
        );
        assert_eq!(
            read <= delay,
            delay != 300,
            "{era}, a delay of {delay}: the emergency keys spend"
        );
    }
}
