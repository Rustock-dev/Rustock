//! rskj `co.rsk.core.bc.BlockExecutorTest`, its strange transactions
//! (`createStrangeTransaction`): which a block can carry, read from the
//! bytes a block holds them in.

use crate::Transaction;
use alloy_primitives::U256;
use alloy_rlp::Encodable;

/// An unsigned transfer, from its nine fields: nonce 0, gas price 1, gas
/// limit 21,000, to twenty bytes, a value of the bytes given, no data and
/// no signature.
fn unsigned_transfer(value: &[u8]) -> Vec<u8> {
    let to = [0x11u8; 20];
    let fields: [&[u8]; 9] = [&[], &[1], &[0x52, 0x08], &to, value, &[], &[], &[], &[]];
    let mut payload = Vec::new();
    for field in fields {
        field.encode(&mut payload);
    }
    let mut raw = Vec::new();
    alloy_rlp::Header {
        list: true,
        payload_length: payload.len(),
    }
    .encode(&mut raw);
    raw.extend_from_slice(&payload);
    raw
}

/// rskj `executeBlocksWithOneStrangeTransactions3`: a value written in
/// 1,024 bytes, all but its last zero, reads as its number, and the
/// transaction is kept, and hashed, as the one writing it in one byte.
/// Its first two, a `to` of one byte and of 1,024, can't be read
/// (`what_rskj_cannot_take_is_refused_when_read`, transaction.rs).
#[test]
fn a_value_padded_with_zeros_reads_as_its_number() {
    let padded = [vec![0u8; 1023], vec![10]].concat();
    let read = Transaction::from_rskj(&unsigned_transfer(&padded)).unwrap();
    let one_byte = Transaction::from_rskj(&unsigned_transfer(&[10])).unwrap();
    assert_eq!(read.value, U256::from(10));
    assert_eq!(read.cached_rlp, one_byte.cached_rlp);
    assert_eq!(read.tx_hash(), one_byte.tx_hash());
}
