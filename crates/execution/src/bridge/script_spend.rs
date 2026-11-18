//! bitcoinj-thin's `Script.correctlySpends`, as rskj's
//! `PegUtilsLegacy.scriptCorrectlySpendsTx` runs it: whether an input of a
//! Bitcoin transaction spends a script, its signatures checked.
//!
//! rskj asks it of each input of a transaction it reads by the legacy rules:
//! whether it spends the old federation (`txIsFromOldFederation`, from
//! RSKIP199), a live federation or the last retired one (`isValidPegInTx`).
//! Each script asked about is a P2SH script, and the redeem script an input
//! pushes must hash to it, so what runs is a scriptSig, which holds pushes
//! and small numbers only, the P2SH script, and a federation's redeem script:
//! a multisig, an ERP script, or a P2SH-P2WSH federation's witness program,
//! run without its witness. This runs the opcodes those hold as bitcoinj runs
//! them, with every flag bitcoinj has (`Script.ALL_VERIFY_FLAGS`). bitcoinj
//! runs the rest of Bitcoin's opcodes too; none is in a script asked about,
//! and here any of them fails the script.

use std::collections::VecDeque;

use bitcoin::{Amount, ScriptBuf, Transaction, TxOut};
use sha2::Digest as _;

const OP_0: u8 = 0x00;
const OP_PUSHDATA1: u8 = 0x4c;
const OP_PUSHDATA2: u8 = 0x4d;
const OP_PUSHDATA4: u8 = 0x4e;
const OP_1NEGATE: u8 = 0x4f;
const OP_1: u8 = 0x51;
const OP_16: u8 = 0x60;
const OP_NOP: u8 = 0x61;
const OP_IF: u8 = 0x63;
const OP_NOTIF: u8 = 0x64;
const OP_VERIF: u8 = 0x65;
const OP_VERNOTIF: u8 = 0x66;
const OP_ELSE: u8 = 0x67;
const OP_ENDIF: u8 = 0x68;
const OP_VERIFY: u8 = 0x69;
const OP_DROP: u8 = 0x75;
const OP_DUP: u8 = 0x76;
const OP_EQUAL: u8 = 0x87;
const OP_EQUALVERIFY: u8 = 0x88;
const OP_HASH160: u8 = 0xa9;
const OP_CODESEPARATOR: u8 = 0xab;
const OP_CHECKMULTISIG: u8 = 0xae;
const OP_CHECKMULTISIGVERIFY: u8 = 0xaf;
const OP_CHECKSEQUENCEVERIFY: u8 = 0xb2;

/// The opcodes bitcoinj refuses wherever they stand, run or not: OP_CAT to
/// OP_RIGHT, OP_INVERT to OP_XOR, OP_2MUL, OP_2DIV and OP_MUL to OP_RSHIFT.
const DISABLED: [u8; 15] = [
    0x7e, 0x7f, 0x80, 0x81, 0x83, 0x84, 0x85, 0x86, 0x8d, 0x8e, 0x95, 0x96, 0x97, 0x98, 0x99,
];

/// `BtcTransaction.SEQUENCE_LOCKTIME_DISABLE_FLAG`.
const SEQUENCE_LOCKTIME_DISABLE_FLAG: i64 = 1 << 31;
/// `BtcTransaction.SEQUENCE_LOCKTIME_TYPE_FLAG`.
const SEQUENCE_LOCKTIME_TYPE_FLAG: i64 = 1 << 22;
/// `BtcTransaction.SEQUENCE_LOCKTIME_MASK`.
const SEQUENCE_LOCKTIME_MASK: i64 = 0xffff;

/// bitcoinj's `ScriptException`: the script fails.
struct Failed;

/// A chunk of a script as bitcoinj's `ScriptParser` reads it.
struct Chunk {
    opcode: u8,
    /// What a push pushes; nothing for any other opcode.
    data: Option<Vec<u8>>,
    /// Where the chunk starts in its program.
    start: usize,
}

impl Chunk {
    /// `ScriptChunk.isOpCode`: anything but a push, so `OP_1NEGATE` and the
    /// small numbers too.
    fn is_op_code(&self) -> bool {
        self.opcode > OP_PUSHDATA4
    }
}

/// `ScriptParser.parseScriptProgram`; `None` is its `ScriptException`, a push
/// that runs past the end.
fn parse(program: &[u8]) -> Option<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut pos = 0;
    while pos < program.len() {
        let start = pos;
        let opcode = program[pos];
        pos += 1;
        let len = match opcode {
            o if o < OP_PUSHDATA1 => Some(o as usize),
            OP_PUSHDATA1 => {
                let n = *program.get(pos)?;
                pos += 1;
                Some(n as usize)
            }
            OP_PUSHDATA2 => {
                let n = program.get(pos..pos + 2)?;
                pos += 2;
                Some(u16::from_le_bytes([n[0], n[1]]) as usize)
            }
            OP_PUSHDATA4 => {
                let n = program.get(pos..pos + 4)?;
                pos += 4;
                Some(u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)
            }
            _ => None,
        };
        let data = match len {
            None => None,
            Some(n) if n > program.len() - pos => return None,
            Some(n) => {
                pos += n;
                Some(program[pos - n..pos].to_vec())
            }
        };
        chunks.push(Chunk {
            opcode,
            data,
            start,
        });
    }
    Some(chunks)
}

/// rskj `PegUtilsLegacy.scriptCorrectlySpendsTx`: whether input `index` of
/// `tx` spends `script_pubkey`. A scriptSig bitcoinj can't parse, or that
/// holds an opcode beyond the small numbers, spends nothing, and so does one
/// that fails as bitcoinj runs it.
pub(crate) fn script_correctly_spends_tx(
    tx: &Transaction,
    index: usize,
    script_pubkey: &[u8],
) -> bool {
    let script_sig = tx.input[index].script_sig.as_bytes();
    let Some(sig_chunks) = parse(script_sig) else {
        return false;
    };
    if sig_chunks
        .iter()
        .any(|c| c.is_op_code() && c.opcode > OP_16)
    {
        return false;
    }
    correctly_spends(tx, index, script_sig, &sig_chunks, script_pubkey).is_ok()
}

/// `Script.correctlySpends` with every flag: the scriptSig run, then the
/// script it spends on what the scriptSig left, and for a P2SH script the
/// redeem script the scriptSig pushed last, on what the scriptSig left beneath
/// it. Each must leave a true value on top.
fn correctly_spends(
    tx: &Transaction,
    index: usize,
    script_sig: &[u8],
    sig_chunks: &[Chunk],
    script_pubkey: &[u8],
) -> Result<(), Failed> {
    if script_sig.len() > 10_000 || script_pubkey.len() > 10_000 {
        return Err(Failed);
    }
    let pubkey_chunks = parse(script_pubkey).ok_or(Failed)?;
    let mut stack = Vec::new();
    execute(tx, index, script_sig, sig_chunks, &mut stack)?;
    let mut p2sh_stack = stack.clone();
    execute(tx, index, script_pubkey, &pubkey_chunks, &mut stack)?;
    if !cast_to_bool(&stack.pop().ok_or(Failed)?) {
        return Err(Failed);
    }
    if is_pay_to_script_hash(script_pubkey) {
        if sig_chunks
            .iter()
            .any(|c| c.is_op_code() && c.opcode > OP_16)
        {
            return Err(Failed);
        }
        let redeem = p2sh_stack.pop().ok_or(Failed)?;
        let redeem_chunks = parse(&redeem).ok_or(Failed)?;
        execute(tx, index, &redeem, &redeem_chunks, &mut p2sh_stack)?;
        if !cast_to_bool(&p2sh_stack.pop().ok_or(Failed)?) {
            return Err(Failed);
        }
    }
    Ok(())
}

/// `Script.isPayToScriptHash`: the exact bytes of `OP_HASH160 <20> OP_EQUAL`.
fn is_pay_to_script_hash(script: &[u8]) -> bool {
    script.len() == 23 && script[0] == OP_HASH160 && script[1] == 0x14 && script[22] == OP_EQUAL
}

/// `Script.executeScript` with every flag.
fn execute(
    tx: &Transaction,
    index: usize,
    program: &[u8],
    chunks: &[Chunk],
    stack: &mut Vec<Vec<u8>>,
) -> Result<(), Failed> {
    let mut op_count = 0usize;
    let mut last_code_separator = 0usize;
    let mut if_stack: Vec<bool> = Vec::new();
    for chunk in chunks {
        let should_execute = !if_stack.contains(&false);
        if chunk.opcode == OP_0 {
            if should_execute {
                stack.push(Vec::new());
            }
        } else if !chunk.is_op_code() {
            let data = chunk.data.as_deref().unwrap_or_default();
            if data.len() > 520 {
                return Err(Failed);
            }
            if should_execute {
                stack.push(data.to_vec());
            }
        } else {
            let opcode = chunk.opcode;
            if opcode > OP_16 {
                op_count += 1;
                if op_count > 201 {
                    return Err(Failed);
                }
            }
            if opcode == OP_VERIF || opcode == OP_VERNOTIF || DISABLED.contains(&opcode) {
                return Err(Failed);
            }
            match opcode {
                OP_IF | OP_NOTIF => {
                    if should_execute {
                        let top = stack.pop().ok_or(Failed)?;
                        if_stack.push(cast_to_bool(&top) == (opcode == OP_IF));
                    } else {
                        if_stack.push(false);
                    }
                    continue;
                }
                OP_ELSE => {
                    let branch = if_stack.pop().ok_or(Failed)?;
                    if_stack.push(!branch);
                    continue;
                }
                OP_ENDIF => {
                    if_stack.pop().ok_or(Failed)?;
                    continue;
                }
                _ => {}
            }
            if !should_execute {
                continue;
            }
            match opcode {
                OP_1NEGATE => stack.push(vec![0x81]),
                OP_1..=OP_16 => stack.push(vec![opcode - OP_1 + 1]),
                OP_NOP => {}
                OP_VERIFY => {
                    if !cast_to_bool(&stack.pop().ok_or(Failed)?) {
                        return Err(Failed);
                    }
                }
                OP_DROP => {
                    stack.pop().ok_or(Failed)?;
                }
                OP_DUP => {
                    let top = stack.last().ok_or(Failed)?.clone();
                    stack.push(top);
                }
                OP_EQUAL | OP_EQUALVERIFY => {
                    let (Some(a), Some(b)) = (stack.pop(), stack.pop()) else {
                        return Err(Failed);
                    };
                    if opcode == OP_EQUAL {
                        stack.push(if a == b { vec![1] } else { Vec::new() });
                    } else if a != b {
                        return Err(Failed);
                    }
                }
                OP_HASH160 => {
                    let top = stack.pop().ok_or(Failed)?;
                    stack.push(hash160(&top).to_vec());
                }
                OP_CODESEPARATOR => last_code_separator = chunk.start + 1,
                OP_CHECKMULTISIG | OP_CHECKMULTISIGVERIFY => {
                    let connected = &program[last_code_separator.min(program.len())..];
                    op_count = check_multisig(tx, index, connected, opcode, stack, op_count)?;
                }
                OP_CHECKSEQUENCEVERIFY => check_sequence_verify(tx, index, stack)?,
                _ => return Err(Failed),
            }
        }
        if stack.len() > 1000 {
            return Err(Failed);
        }
    }
    if !if_stack.is_empty() {
        return Err(Failed);
    }
    Ok(())
}

/// `Script.castToBool`: false for nothing but zeros, or zeros ending in the
/// negative zero 0x80.
fn cast_to_bool(data: &[u8]) -> bool {
    match data.iter().position(|b| *b != 0) {
        None => false,
        Some(i) => !(i == data.len() - 1 && data[i] == 0x80),
    }
}

/// `Utils.decodeMPI(Utils.reverseBytes(data), false)`: a number written
/// little-endian, the top bit of its last byte its sign.
fn script_number(data: &[u8]) -> i64 {
    let Some((last, rest)) = data.split_last() else {
        return 0;
    };
    let magnitude = rest
        .iter()
        .chain(std::iter::once(&(last & 0x7f)))
        .enumerate()
        .fold(0i64, |n, (i, b)| n | (i64::from(*b) << (8 * i)));
    if last & 0x80 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

/// `castToBigInteger(data).intValue()`: a number of at most four bytes.
fn small_number(data: &[u8]) -> Result<i64, Failed> {
    if data.len() > 4 {
        return Err(Failed);
    }
    Ok(script_number(data))
}

/// `Script.executeMultiSig`: the key count, the keys, the signature count and
/// the signatures popped, each signature taken out of the script it is checked
/// against, and the signatures matched to the keys in order from the last of
/// each, a key passed over for good once it fails one. Then the extra item
/// `OP_CHECKMULTISIG` pops, which must be empty (NULLDUMMY).
fn check_multisig(
    tx: &Transaction,
    index: usize,
    connected: &[u8],
    opcode: u8,
    stack: &mut Vec<Vec<u8>>,
    mut op_count: usize,
) -> Result<usize, Failed> {
    if stack.len() < 2 {
        return Err(Failed);
    }
    let key_count = small_number(&stack.pop().ok_or(Failed)?)?;
    if !(0..=20).contains(&key_count) {
        return Err(Failed);
    }
    let key_count = key_count as usize;
    op_count += key_count;
    if op_count > 201 || stack.len() < key_count + 1 {
        return Err(Failed);
    }
    let mut keys: VecDeque<Vec<u8>> = (0..key_count).filter_map(|_| stack.pop()).collect();
    let sig_count = small_number(&stack.pop().ok_or(Failed)?)?;
    if sig_count < 0 || sig_count as usize > key_count {
        return Err(Failed);
    }
    let sig_count = sig_count as usize;
    if stack.len() < sig_count + 1 {
        return Err(Failed);
    }
    let mut sigs: VecDeque<Vec<u8>> = (0..sig_count).filter_map(|_| stack.pop()).collect();
    let connected = sigs.iter().fold(connected.to_vec(), |script, sig| {
        remove_all_instances_of(&script, &push_of(sig))
    });
    let mut valid = true;
    while let Some(sig) = sigs.front() {
        if let Some(key) = keys.pop_front() {
            if signature_verifies(tx, index, &connected, sig, &key) {
                sigs.pop_front();
            }
        }
        if sigs.len() > keys.len() {
            valid = false;
            break;
        }
    }
    let dummy = stack.pop().ok_or(Failed)?;
    if !dummy.is_empty() {
        return Err(Failed);
    }
    if opcode == OP_CHECKMULTISIG {
        stack.push(if valid { vec![1] } else { Vec::new() });
    } else if !valid {
        return Err(Failed);
    }
    Ok(op_count)
}

/// `Script.writeBytes`: `data` as a push.
fn push_of(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 3);
    match data.len() {
        n if n < OP_PUSHDATA1 as usize => out.push(n as u8),
        n if n < 256 => out.extend_from_slice(&[OP_PUSHDATA1, n as u8]),
        n => {
            out.push(OP_PUSHDATA2);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
    }
    out.extend_from_slice(data);
    out
}

/// `Script.removeAllInstancesOf`: `script` without each chunk whose bytes start
/// with `remove`, the chunks walked as bitcoinj walks them.
fn remove_all_instances_of(script: &[u8], remove: &[u8]) -> Vec<u8> {
    let byte = |i: usize| script.get(i).map_or(0, |b| *b as usize);
    let mut out = Vec::with_capacity(script.len());
    let mut cursor = 0;
    while cursor < script.len() {
        let skip = script[cursor..].starts_with(remove);
        let opcode = script[cursor];
        cursor += 1;
        let additional = match opcode {
            o if o < OP_PUSHDATA1 => o as usize,
            OP_PUSHDATA1 => byte(cursor) + 1,
            OP_PUSHDATA2 => (byte(cursor) | (byte(cursor + 1) << 8)) + 2,
            // bitcoinj reads the length's second byte three times over.
            OP_PUSHDATA4 => {
                (byte(cursor)
                    | (byte(cursor + 1) << 8)
                    | (byte(cursor + 1) << 16)
                    | (byte(cursor + 1) << 24))
                    + 4
            }
            _ => 0,
        };
        if !skip {
            out.push(opcode);
            out.extend_from_slice(
                &script[cursor.min(script.len())..(cursor + additional).min(script.len())],
            );
        }
        cursor += additional;
    }
    out
}

/// One turn of `executeMultiSig`'s loop: the signature read as
/// `TransactionSignature.decodeFromBitcoin` reads it, its encoding required
/// canonical and its S not required low, the transaction hashed for its
/// sighash byte, and the key checked as Bouncy Castle's `ECDSASigner` checks
/// one. What throws on the way is a signature that doesn't verify.
fn signature_verifies(
    tx: &Transaction,
    index: usize,
    connected: &[u8],
    sig: &[u8],
    key: &[u8],
) -> bool {
    if !is_encoding_canonical(sig) {
        return false;
    }
    let len_r = sig[3] as usize;
    let r = &sig[4..4 + len_r];
    let len_s = sig[5 + len_r] as usize;
    let s = &sig[6 + len_r..6 + len_r + len_s];
    let hash = signature_hash(tx, index, connected, sig[sig.len() - 1]);
    verify(&hash, r, s, key)
}

/// `TransactionSignature.isEncodingCanonical`: DER's sequence of two
/// integers, neither negative nor padded, then a sighash byte that is ALL,
/// NONE or SINGLE with or without ANYONECANPAY.
fn is_encoding_canonical(sig: &[u8]) -> bool {
    if sig.len() < 9 || sig.len() > 73 {
        return false;
    }
    let hash_type = sig[sig.len() - 1] & !0x80;
    if !(1..=3).contains(&hash_type) {
        return false;
    }
    if sig[0] != 0x30 || sig[1] as usize != sig.len() - 3 {
        return false;
    }
    let len_r = sig[3] as usize;
    if 5 + len_r >= sig.len() || len_r == 0 {
        return false;
    }
    let len_s = sig[5 + len_r] as usize;
    if len_r + len_s + 7 != sig.len() || len_s == 0 {
        return false;
    }
    if sig[2] != 0x02 || sig[4] & 0x80 != 0 {
        return false;
    }
    if len_r > 1 && sig[4] == 0 && sig[5] & 0x80 == 0 {
        return false;
    }
    if sig[4 + len_r] != 0x02 || sig[6 + len_r] & 0x80 != 0 {
        return false;
    }
    if len_s > 1 && sig[6 + len_r] == 0 && sig[7 + len_r] & 0x80 == 0 {
        return false;
    }
    true
}

/// Bouncy Castle's `ECDSASigner.verifySignature` over a 32-byte hash: R and S
/// read unsigned, each from 1 to the order less one, and a high S taken as the
/// low one it mirrors. A key that doesn't decode verifies nothing.
fn verify(hash: &[u8; 32], r: &[u8], s: &[u8], key: &[u8]) -> bool {
    use k256::ecdsa::signature::hazmat::PrehashVerifier;
    let scalar = |bytes: &[u8]| -> Option<[u8; 32]> {
        let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
        let digits = &bytes[start..];
        if digits.len() > 32 {
            return None;
        }
        let mut out = [0u8; 32];
        out[32 - digits.len()..].copy_from_slice(digits);
        Some(out)
    };
    let (Some(r), Some(s)) = (scalar(r), scalar(s)) else {
        return false;
    };
    let Ok(signature) = k256::ecdsa::Signature::from_scalars(r, s) else {
        return false;
    };
    let signature = signature.normalize_s().unwrap_or(signature);
    let Ok(key) = k256::ecdsa::VerifyingKey::from_sec1_bytes(key) else {
        return false;
    };
    key.verify_prehash(hash, &signature).is_ok()
}

/// `BtcTransaction.hashForSignature(index, connectedScript, sigHashType)`:
/// Bitcoin's legacy signature hash, but for the copy's serialization, which
/// bitcoinj writes as it writes every transaction, with its witnesses when it
/// has any. Under ANYONECANPAY the copy keeps the signing input alone and,
/// since bitcoinj keeps a transaction's witnesses by position, the first
/// input's witness.
fn signature_hash(tx: &Transaction, index: usize, connected: &[u8], sighash_type: u8) -> [u8; 32] {
    let mut copy = tx.clone();
    for input in &mut copy.input {
        input.script_sig = ScriptBuf::new();
    }
    copy.input[index].script_sig =
        ScriptBuf::from_bytes(remove_all_instances_of(connected, &[OP_CODESEPARATOR]));
    let others_sequence_zeroed = |copy: &mut Transaction| {
        for (i, input) in copy.input.iter_mut().enumerate() {
            if i != index {
                input.sequence = bitcoin::Sequence(0);
            }
        }
    };
    match sighash_type & 0x1f {
        2 => {
            copy.output.clear();
            others_sequence_zeroed(&mut copy);
        }
        3 => {
            if index >= copy.output.len() {
                let mut one = [0u8; 32];
                one[0] = 1;
                return one;
            }
            copy.output.truncate(index + 1);
            for output in &mut copy.output[..index] {
                *output = TxOut {
                    value: Amount::from_sat(u64::MAX),
                    script_pubkey: ScriptBuf::new(),
                };
            }
            others_sequence_zeroed(&mut copy);
        }
        _ => {}
    }
    if sighash_type & 0x80 != 0 {
        let mut input = copy.input[index].clone();
        input.witness = copy.input[0].witness.clone();
        copy.input = vec![input];
    }
    let mut bytes = bitcoin::consensus::serialize(&copy);
    bytes.extend_from_slice(&u32::from(sighash_type).to_le_bytes());
    sha2::Sha256::digest(sha2::Sha256::digest(&bytes)).into()
}

/// `Script.executeCheckSequenceVerify`: the number on top of the stack, of at
/// most five bytes and not negative, against the input's sequence as BIP112
/// compares them, unless it has the disable flag.
fn check_sequence_verify(tx: &Transaction, index: usize, stack: &[Vec<u8>]) -> Result<(), Failed> {
    let top = stack.last().ok_or(Failed)?;
    if top.len() > 5 {
        return Err(Failed);
    }
    let sequence = script_number(top);
    if sequence < 0 {
        return Err(Failed);
    }
    if sequence & SEQUENCE_LOCKTIME_DISABLE_FLAG != 0 {
        return Ok(());
    }
    let tx_sequence = i64::from(tx.input[index].sequence.0);
    if (tx.version.0 as u32) < 2 || tx_sequence & SEQUENCE_LOCKTIME_DISABLE_FLAG != 0 {
        return Err(Failed);
    }
    let mask = SEQUENCE_LOCKTIME_TYPE_FLAG | SEQUENCE_LOCKTIME_MASK;
    let (tx_masked, masked) = (tx_sequence & mask, sequence & mask);
    if (tx_masked < SEQUENCE_LOCKTIME_TYPE_FLAG) != (masked < SEQUENCE_LOCKTIME_TYPE_FLAG)
        || masked > tx_masked
    {
        return Err(Failed);
    }
    Ok(())
}

/// RIPEMD160(SHA256(data)).
fn hash160(data: &[u8]) -> [u8; 20] {
    use ripemd::Digest as _;
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}

#[cfg(test)]
mod tests {
    use bitcoin::hashes::Hash as _;
    use k256::ecdsa::signature::hazmat::PrehashSigner as _;
    use k256::ecdsa::{Signature, SigningKey};

    use super::*;

    fn key_of(member: &SigningKey) -> [u8; 33] {
        member
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .try_into()
            .unwrap()
    }

    /// The keys of the private keys `[seed; 32]`, in their public keys' order.
    fn signers(seeds: std::ops::RangeInclusive<u8>) -> Vec<SigningKey> {
        let mut signers: Vec<SigningKey> = seeds
            .map(|seed| SigningKey::from_slice(&[seed; 32]).unwrap())
            .collect();
        signers.sort_by_key(key_of);
        signers
    }

    /// `data` pushed as bitcoinj pushes it.
    fn push(data: &[u8]) -> Vec<u8> {
        let mut out = if data.len() < usize::from(OP_PUSHDATA1) {
            vec![data.len() as u8]
        } else {
            vec![OP_PUSHDATA1, data.len() as u8]
        };
        out.extend_from_slice(data);
        out
    }

    /// `OP_2 <keys> OP_3 OP_CHECKMULTISIG`.
    fn multisig(members: &[SigningKey]) -> Vec<u8> {
        let mut script = vec![0x52];
        for member in members {
            script.extend(push(&key_of(member)));
        }
        script.extend([0x53, OP_CHECKMULTISIG]);
        script
    }

    /// A P2SH ERP script: `members`' multisig, or once the input's sequence
    /// counts `delay`, `emergency`'s.
    fn p2sh_erp(members: &[SigningKey], emergency: &[SigningKey], delay: &[u8]) -> Vec<u8> {
        [
            vec![OP_NOTIF],
            multisig(members),
            vec![OP_ELSE],
            push(delay),
            vec![OP_CHECKSEQUENCEVERIFY, OP_DROP],
            multisig(emergency),
            vec![OP_ENDIF],
        ]
        .concat()
    }

    fn p2sh(redeem: &[u8]) -> Vec<u8> {
        [&[OP_HASH160, 0x14][..], &hash160(redeem), &[OP_EQUAL]].concat()
    }

    /// A version 2 transaction whose first input has the sequence `sequence`,
    /// followed by `witnessed` inputs with a witness each.
    fn transaction(sequence: u32, witnessed: u32) -> Transaction {
        let input = |vout: u32, sequence: u32, witness: bitcoin::Witness| bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([0x5a; 32]),
                vout,
            },
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence(sequence),
            witness,
        };
        let witness = || bitcoin::Witness::from_slice(&[vec![0x42; 72], vec![0x02; 33]]);
        Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: std::iter::once(input(0, sequence, bitcoin::Witness::new()))
                .chain((1..=witnessed).map(|vout| input(vout, u32::MAX, witness())))
                .collect(),
            output: vec![TxOut {
                value: Amount::from_sat(90_000),
                script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
            }],
        }
    }

    /// Bitcoin's legacy signature hash of the first input for SIGHASH_ALL.
    fn bitcoin_sighash(tx: &Transaction, redeem: &[u8]) -> [u8; 32] {
        bitcoin::sighash::SighashCache::new(tx)
            .legacy_signature_hash(0, bitcoin::Script::from_bytes(redeem), 1)
            .unwrap()
            .to_byte_array()
    }

    /// `hash` signed by `signer`, DER-encoded, with the sighash byte ALL.
    fn signature(signer: &SigningKey, hash: &[u8; 32]) -> Vec<u8> {
        let signature: Signature = signer.sign_prehash(hash).unwrap();
        [signature.to_der().as_bytes(), &[0x01]].concat()
    }

    /// Whether the first input of `tx`, its scriptSig `items` and then
    /// `redeem` pushed, spends `redeem`'s P2SH script.
    fn spends(tx: &Transaction, items: &[&[u8]], redeem: &[u8]) -> bool {
        let mut tx = tx.clone();
        let mut script_sig = Vec::new();
        for item in items {
            script_sig.extend(if item.is_empty() {
                vec![OP_0]
            } else {
                push(item)
            });
        }
        script_sig.extend(push(redeem));
        tx.input[0].script_sig = ScriptBuf::from_bytes(script_sig);
        script_correctly_spends_tx(&tx, 0, &p2sh(redeem))
    }

    /// A multisig spend that two members sign in their keys' order spends
    /// it; signed in the other order, or by a key not a member's, it doesn't.
    #[test]
    fn two_members_signing_in_their_keys_order_spend_a_multisig() {
        let members = signers(1..=3);
        let redeem = multisig(&members);
        let tx = transaction(u32::MAX, 0);
        let hash = bitcoin_sighash(&tx, &redeem);
        let (first, third) = (signature(&members[0], &hash), signature(&members[2], &hash));
        assert!(spends(&tx, &[&[], &first, &third], &redeem));
        assert!(!spends(&tx, &[&[], &third, &first], &redeem));
        let outsider = signature(&SigningKey::from_slice(&[9; 32]).unwrap(), &hash);
        assert!(!spends(&tx, &[&[], &first, &outsider], &redeem));
    }

    /// bitcoinj checks a multisig's signatures with S high as with S low, and
    /// refuses one whose encoding isn't DER's shortest, or whose sighash byte
    /// is none of ALL, NONE and SINGLE.
    #[test]
    fn a_high_s_verifies_and_an_encoding_not_canonical_does_not() {
        let members = signers(1..=3);
        let redeem = multisig(&members);
        let tx = transaction(u32::MAX, 0);
        let hash = bitcoin_sighash(&tx, &redeem);
        let first = signature(&members[0], &hash);
        let low: Signature = members[1].sign_prehash(&hash).unwrap();
        let low = low.normalize_s().unwrap_or(low);
        let high = Signature::from_scalars((*low.r()).to_bytes(), (-*low.s()).to_bytes()).unwrap();
        let encoded = |signature: &Signature, sighash: u8| {
            [signature.to_der().as_bytes(), &[sighash]].concat()
        };
        assert!(spends(&tx, &[&[], &first, &encoded(&low, 0x01)], &redeem));
        assert!(spends(&tx, &[&[], &first, &encoded(&high, 0x01)], &redeem));
        assert!(!spends(&tx, &[&[], &first, &encoded(&low, 0x04)], &redeem));
        // R behind a zero it doesn't need.
        let der = low.to_der();
        let der = der.as_bytes();
        let padded = [
            &[0x30, der[1] + 1, 0x02, der[3] + 1, 0x00][..],
            &der[4..],
            &[0x01],
        ]
        .concat();
        assert!(!spends(&tx, &[&[], &first, &padded], &redeem));
    }

    /// `OP_CHECKMULTISIG` pops an item more than its signatures, which must be
    /// empty.
    #[test]
    fn a_multisig_needs_its_extra_item_empty() {
        let members = signers(1..=3);
        let redeem = multisig(&members);
        let tx = transaction(u32::MAX, 0);
        let hash = bitcoin_sighash(&tx, &redeem);
        let (first, second) = (signature(&members[0], &hash), signature(&members[1], &hash));
        assert!(spends(&tx, &[&[], &first, &second], &redeem));
        assert!(!spends(&tx, &[&[0x01], &first, &second], &redeem));
    }

    /// bitcoinj hashes a transaction for a legacy signature as it serializes
    /// it, with its witnesses where it has any: where another input has one, a
    /// signature over that hash verifies and one over Bitcoin's doesn't.
    #[test]
    fn the_signature_hash_covers_the_witnesses_of_a_transaction_with_any() {
        let members = signers(1..=3);
        let redeem = multisig(&members);
        let tx = transaction(u32::MAX, 1);
        let mut copy = tx.clone();
        copy.input[0].script_sig = ScriptBuf::from_bytes(redeem.clone());
        let mut preimage = bitcoin::consensus::serialize(&copy);
        preimage.extend_from_slice(&1u32.to_le_bytes());
        let with_witnesses: [u8; 32] = sha2::Sha256::digest(sha2::Sha256::digest(&preimage)).into();
        let without = bitcoin_sighash(&tx, &redeem);
        assert_ne!(with_witnesses, without);
        let signed_over = |hash: &[u8; 32]| {
            let (first, second) = (signature(&members[0], hash), signature(&members[1], hash));
            spends(&tx, &[&[], &first, &second], &redeem)
        };
        assert!(signed_over(&with_witnesses));
        assert!(!signed_over(&without));
    }

    /// An ERP script's members spend it at once. Its emergency keys spend it
    /// once the input's sequence counts its delay, in a transaction of version
    /// 2 or more, as BIP112 has it (bitcoinj's `checkSequence`).
    #[test]
    fn an_erp_scripts_emergency_keys_wait_for_its_delay() {
        let (members, emergency) = (signers(1..=3), signers(4..=6));
        let redeem = p2sh_erp(&members, &emergency, &[0xf4, 0x01]);
        let signed = |tx: &Transaction, signers: &[SigningKey], branch: &[u8]| {
            let hash = bitcoin_sighash(tx, &redeem);
            let (first, second) = (signature(&signers[0], &hash), signature(&signers[1], &hash));
            spends(tx, &[&[], &first, &second, branch], &redeem)
        };
        assert!(signed(&transaction(0, 0), &members, &[]));
        assert!(!signed(&transaction(0, 0), &emergency, &[]));
        assert!(signed(&transaction(500, 0), &emergency, &[0x01]));
        assert!(!signed(&transaction(499, 0), &emergency, &[0x01]));
        let mut version_1 = transaction(500, 0);
        version_1.version = bitcoin::transaction::Version::ONE;
        assert!(!signed(&version_1, &emergency, &[0x01]));
    }

    /// `scriptCorrectlySpendsTx` takes no scriptSig holding an opcode beyond
    /// the small numbers, whatever it would do run, nor one bitcoinj can't
    /// parse.
    #[test]
    fn a_script_sig_of_more_than_pushes_spends_nothing() {
        let members = signers(1..=3);
        let redeem = multisig(&members);
        let mut tx = transaction(u32::MAX, 0);
        let hash = bitcoin_sighash(&tx, &redeem);
        let (first, second) = (signature(&members[0], &hash), signature(&members[1], &hash));
        let pushed = [vec![OP_0], push(&first), push(&second)].concat();
        for script_sig in [
            [&pushed[..], &[OP_NOP], &push(&redeem)].concat(),
            [&pushed[..], &push(&redeem)[..redeem.len()]].concat(),
        ] {
            tx.input[0].script_sig = ScriptBuf::from_bytes(script_sig);
            assert!(!script_correctly_spends_tx(&tx, 0, &p2sh(&redeem)));
        }
    }
}
