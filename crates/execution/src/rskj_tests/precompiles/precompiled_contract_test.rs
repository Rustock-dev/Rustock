//! rskj's `org.ethereum.vm.PrecompiledContractTest`, its tests of BLAKE2F
//! (RSKIP153, iris300): EIP-152's test vectors, run as a contract's call of
//! BLAKE2F in a block of mainnet.

use alloy_primitives::{hex, Address};

use super::harness::call_precompile;

/// iris300 on mainnet: RSKIP153.
const IRIS: u64 = 3_614_800;
/// BLAKE2F.
const BLAKE2F: Address = Address::with_last_byte(9);

/// EIP-152's input of `rounds` rounds and the final-block flag `last`:
/// BLAKE2b-512's state h, the message "abc", and an offset of 3.
fn vector(rounds: u32, last: u8) -> Vec<u8> {
    let mut input = rounds.to_be_bytes().to_vec();
    let state = [
        "48c9bdf267e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5",
        "d182e6ad7f520e511f6c3e2b8c68059b6bbd41fbabd9831f79217e1319cde05b",
    ];
    input.extend(hex::decode(state.concat()).expect("hex"));
    let mut message = [0u8; 128];
    message[..3].copy_from_slice(b"abc");
    input.extend(message);
    input.extend([3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    input.push(last);
    input
}

/// An input BLAKE2F refuses fails the call: one of no bytes, of a byte short
/// of the 213 BLAKE2F reads, of a byte past them, and of a final-block flag
/// of 2.
#[test]
fn blake2f_refuses_an_input_of_the_wrong_length_or_flag() {
    let twelve = vector(12, 1);
    let cases = [
        ("blake2fTestVector0", Vec::new()),
        ("blake2fTestVector1", twelve[1..].to_vec()),
        ("blake2fTestVector2", [&[0u8][..], &twelve].concat()),
        ("blake2fTestVector3", vector(12, 2)),
    ];
    for (test, input) in cases {
        let ran = call_precompile(IRIS, BLAKE2F, 1_000, &input);
        assert_eq!(ran, (false, Vec::new()), "{test}");
    }
}

/// BLAKE2F compresses EIP-152's vectors to their hashes, for a gas a round:
/// given one gas less than its rounds, the call fails.
#[test]
fn blake2f_compresses_eip152s_vectors_for_a_gas_a_round() {
    let cases = [
        (
            "blake2fTestVector4",
            vector(0, 1),
            "08c9bcf367e6096a3ba7ca8485ae67bb2bf894fe72f36e3cf1361d5f3af54fa5\
             d282e6ad7f520e511f6c3e2b8c68059b9442be0454267ce079217e1319cde05b",
        ),
        (
            "blake2fTestVector5",
            vector(12, 1),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923",
        ),
        (
            "blake2fTestVector6",
            vector(12, 0),
            "75ab69d3190a562c51aef8d88f1c2775876944407270c42c9844252c26d28752\
             98743e7f6d5ea2f2d3e8d226039cd31b4e426ac4f2d3d666a610c2116fde4735",
        ),
        (
            "blake2fTestVector7",
            vector(1, 1),
            "b63a380cb2897d521994a85234ee2c181b5f844d2c624c002677e9703449d2fb\
             a551b3a8333bcdf5f2f7e08993d53923de3d64fcc68c034e717b9293fed7a421",
        ),
    ];
    for (test, input, hash) in cases {
        let rounds = u32::from_be_bytes(input[..4].try_into().expect("four bytes"));
        let hash = hex::decode(hash).expect("hex");
        let ran = call_precompile(IRIS, BLAKE2F, rounds, &input);
        assert_eq!(ran, (true, hash), "{test}");
        if rounds > 0 {
            let short = call_precompile(IRIS, BLAKE2F, rounds - 1, &input);
            assert_eq!(short, (false, Vec::new()), "{test}: a gas short");
        }
    }
}
