//! rskj's `co.rsk.pcc.AltBN128Test`: what the BN128 precompiled contracts
//! (RSKIP137) answer a contract's call with input they refuse, before
//! RSKIP197 (iris300) and from it, and with input they take.
//!
//! Each call is given exactly the contract's price, which rskj's tests hold
//! too: a gas less fails it.

use alloy_primitives::{hex, Address};

use super::harness::call_precompile;

/// iris300 on mainnet: RSKIP197.
const IRIS: u64 = 3_614_800;
/// BN128 addition.
const ADD: Address = Address::with_last_byte(6);
/// BN128 multiplication.
const MUL: Address = Address::with_last_byte(7);
/// BN128 pairing.
const PAIRING: Address = Address::with_last_byte(8);

/// `words` words of 0x11 bytes: rskj's coordinates of no point on the curve.
fn elevens(words: usize) -> Vec<u8> {
    vec![0x11; 32 * words]
}

/// The bytes of `words`, each written in hex.
fn bytes(words: &[&str]) -> Vec<u8> {
    hex::decode(words.concat()).expect("hex")
}

/// What a contract's call of `precompile` on `input`, given `price`, comes
/// to before RSKIP197 and from it, holding first that a gas less fails it.
fn before_and_from(precompile: Address, input: &[u8], price: u32) -> [(bool, Vec<u8>); 2] {
    let short = call_precompile(IRIS - 1, precompile, price - 1, input);
    assert!(!short.0, "{price} less one");
    [IRIS - 1, IRIS].map(|number| call_precompile(number, precompile, price, input))
}

/// Input BN128 refuses: before RSKIP197 the call succeeds with no bytes
/// back, and from it the contract throws and the call fails.
#[test]
fn input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it() {
    let refused = |test: &str, precompile: Address, input: &[u8], price: u32| {
        let [before, from] = before_and_from(precompile, input, price);
        assert_eq!(before, (true, Vec::new()), "{test}");
        assert_eq!(from, (false, Vec::new()), "{test}");
    };
    let test = "testALTBN128AddPointNotOnCurveShouldFail";
    refused(test, ADD, &elevens(4), 150);
    refused("shouldFailForPointNotOnCurve", MUL, &elevens(3), 6_000);
    refused("mulShouldFailForNotEnoughParams", MUL, &elevens(2), 6_000);
    refused("invalidInputForPairing", PAIRING, &[0x11], 45_000);
    // rskj's two pairs of a point of G1 and one of G2, not on the curve.
    let g2 = [
        "1382cd45e5674247f9c900b5c6f6cabbc189c2fabe2df0bf5acd84c97818f508",
        "1246178655ab8f2f26956b189894b7eb93cd4215b9937e7969e44305f80f521e",
        "08331c0a261a74e7e75db1232956663cbc88110f726159c5cba1857ecd03fa64",
        "1fbf8045ce3e79b5cde4112d38bcd0efbdb1295d2eefdf58151ae309d7ded7db",
    ];
    let one = "0000000000000000000000000000000000000000000000000000000000000001";
    let two = "0000000000000000000000000000000000000000000000000000000000000002";
    let minus_two = "30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd45";
    let pairs = bytes(&[&[one, two][..], &g2[..], &[one, minus_two][..], &g2[..]].concat());
    let test = "shouldReturnEmptyForPairingPointsNotOnCurve";
    refused(test, PAIRING, &pairs, 113_000);
}

/// Input BN128 takes gets the same answer before RSKIP197 and from it: a
/// multiplication of nothing is the point at infinity, 64 zero bytes; a
/// pairing of nothing holds, 1; and two valid pairs whose product is not one
/// don't, 0.
#[test]
fn input_bn128_takes_gets_the_same_answer_before_rskip197_and_from_it() {
    let answers = |test: &str, precompile: Address, input: &[u8], price: u32, answer: &[u8]| {
        let [before, from] = before_and_from(precompile, input, price);
        assert_eq!(before, (true, answer.to_vec()), "{test}");
        assert_eq!(from, (true, answer.to_vec()), "{test}");
    };
    let mut one = [0u8; 32];
    one[31] = 1;
    answers("mulShouldFailEmptyParams", MUL, &[], 6_000, &[0; 64]);
    answers("shouldReturnTrueForNullInput", PAIRING, &[], 45_000, &one);
    answers("emptyInputForPairing", PAIRING, &[], 45_000, &one);
    let pairs = bytes(&[
        "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f59",
        "3034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41",
        "209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf7",
        "04bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a41678",
        "2bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d",
        "120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550",
        "111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c",
        "103188585e2364128fe25c70558f1560f4f9350baf3959e603cc91486e110936",
        "198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c2",
        "1800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed",
        "090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b",
        "12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
    ]);
    let test = "shouldReturnFalseForValidPointsPairing";
    answers(test, PAIRING, &pairs, 113_000, &[0; 32]);
}
