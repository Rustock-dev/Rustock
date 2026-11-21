//! Can any deployed contract *reach* a given opcode? A census of the state at
//! one block.
//!
//! The general question this answers: **is a rule change dead code on this
//! chain?** Every opcode-gated hardfork raises it -- did anything ever use
//! `SELFDESTRUCT`, `CREATE2`, `EXTCODEHASH`, a precompile, an opcode a later
//! RSKIP removed -- and the honest answer usually needs the chain rather than
//! an argument. `scan_edges` asks the same shape of question about RSKIP-144
//! from the headers; this asks it from the code.
//!
//! The default arguments are the question it was written for: RSK's own `DUPN`
//! (0xa8), `SWAPN` (0xa9) and `TXINDEX` (0xaa) were live until RSKIP191 removed
//! them at iris300, #3,614,800 on mainnet. Pass `--ops` and `--block` for any
//! other.
//!
//! Re-executing every block below a fork answers such a question exactly, and
//! costs about 4.5 hours on two workers for the pre-iris300 range. This answers
//! the *necessary condition* in one trie walk -- about 17 minutes against the
//! archive -- and a negative is a proof rather than a sample. A positive is the
//! input to a targeted re-execution: re-run only the blocks touching the
//! addresses it names.
//!
//! # Why a byte scan would be wrong, and what makes this one right
//!
//! `grep 0xa8` over bytecode hits almost every contract, because the byte
//! occurs constantly inside `PUSH` immediates and in the metadata trailer. Such
//! an occurrence can never execute.
//!
//! What *can* execute is decided by the EVM's own jumpdest analysis, and that
//! analysis is a linear decode from offset zero that steps over each `PUSH`'s
//! immediate. A `JUMPDEST` byte sitting inside an immediate is not a valid
//! target, so there is no second decoding of the same code: **the positions a
//! linear decode calls instructions are exactly the positions that can ever be
//! executed.** This walks that decode and reports a hit only at such a
//! position. No false negatives, and the false positives are "present and
//! reachable in principle" rather than "executed", which is the honest shape of
//! the answer.
//!
//! # What it covers, and what it cannot
//!
//! The state at one block holds the code of every contract **alive at that
//! block**. It cannot hold the code of a contract that was created and
//! `SELFDESTRUCT`ed earlier. So a clean result here means "no surviving
//! contract could reach it", and closing the question completely needs a second
//! pass over transaction input data -- which catches directly-deployed code
//! including destroyed contracts -- leaving only runtime-computed code that was
//! also destroyed.
//!
//! # A missing node voids the result
//!
//! The scan is only evidence if the walk was complete. Every reference the
//! store cannot resolve is counted, and a non-zero count makes the exit status
//! non-zero and the verdict "incomplete": an absent subtree is exactly where
//! the answer could have been hiding.
//!
//! Read-only. Sharded across threads by work-stealing over subtrees, since the
//! cost is random reads rather than CPU.
//!
//! ```text
//! opcode_census <block-dir> <trie-dir> [--block N] [--ops a8,a9,aa] [--threads N]
//! ```
//!
//! `--block` defaults to #3,614,799, the last block before iris300.

use anyhow::{Context, Result};
use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::{NodeRef, TrieNode, TrieStore};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Last block before RSKIP191 removed the three opcodes on mainnet.
const DEFAULT_BLOCK: u64 = 3_614_799;

fn flag(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|w| w[0] == format!("--{name}")).map(|w| w[1].clone())
}

/// A code leaf's key is `account_key || 0x80`; the address is the tail of the
/// account key. `classify` in `trie_tool` is the reference for the shapes.
fn code_address(key: &[u8]) -> Option<Vec<u8>> {
    match key.len() {
        32 if key[31] == 0x80 => Some(key[11..31].to_vec()), // 0x00 | hash[0:10] | addr
        13 if key[12] == 0x80 => Some(key[11..12].to_vec()), // one-byte address (REMASC)
        _ => None,
    }
}

/// Where solc's metadata trailer starts, if the code carries one.
///
/// The trailer is CBOR plus a two-byte big-endian length, appended *after* the
/// runtime code. It is data, and a linear decode walks straight into it and
/// reports whatever the hash bytes happen to look like. That is not a
/// hypothetical source of noise: CBOR map headers are 0xa1..0xb7, so a
/// metadata blob can begin with the very bytes being looked for.
fn metadata_start(code: &[u8]) -> Option<usize> {
    if code.len() < 3 {
        return None;
    }
    let len = u16::from_be_bytes([code[code.len() - 2], code[code.len() - 1]]) as usize;
    let start = code.len().checked_sub(2 + len)?;
    // A CBOR map of 1..=7 pairs is what solc emits; anything else and the two
    // trailing bytes are not a length and the code has no trailer.
    (len >= 2 && (0xa1..=0xa7).contains(&code[start])).then_some(start)
}

/// Where execution starts, or `None` if the code can execute nothing.
///
/// RSK contracts may carry a code header, intended for selecting a VM version
/// and present under no activation rule, on every network from genesis
/// (rskj `Program.processAndSkipCodeHeader`, mirrored by rustock's
/// `skip_code_header`). Code of four bytes or more beginning with `HEADER`
/// (0xfc) runs from `4 + code[3]`, the extra length read unsigned, and jump
/// destinations are found from there -- so a `JUMPDEST` inside the header is
/// not one.
///
/// Decoding such a contract from offset zero would mis-align every instruction
/// after the header and invent positions that do not exist. That was a real
/// defect in the first version of this tool.
///
/// `None` covers the degenerate forms, which fail with all their gas: a 0xfc
/// with fewer than four bytes behind it, and a header reaching or passing the
/// code's end. rskj sets `pc` past the end and throws on the read; rustock
/// leaves `pc` at 0 on the 0xfc, which is an unassigned opcode. Different
/// mechanism, same outcome -- and either way nothing in such code is reachable.
fn exec_start(code: &[u8]) -> Option<usize> {
    if code.first() != Some(&0xfc) {
        return Some(0);
    }
    if code.len() < 4 {
        return None;
    }
    let start = 4 + code[3] as usize;
    (start < code.len()).then_some(start)
}

/// Offsets at which a target byte can actually be executed.
///
/// Three filters, each sound in the sense that it can only remove positions
/// the EVM could never reach:
///
/// 1. **Linear decode.** A byte inside a `PUSH` immediate is not an
///    instruction, and cannot become one: jumpdest analysis is computed over
///    this same decode, so a `JUMPDEST` hidden in an immediate is not a valid
///    target and there is no second decoding of the code.
/// 2. **The metadata trailer is excluded.** It is appended data, not code.
/// 3. **Dead regions after a terminator are excluded.** Control flow cannot
///    fall through `STOP`, `RETURN`, `REVERT`, `INVALID`, `SELFDESTRUCT` or an
///    unconditional `JUMP`, so bytes after one are unreachable until the next
///    `JUMPDEST` -- which is where a jump could land. This is what removes
///    constant pools and padding, and it is conservative: every `JUMPDEST` is
///    treated as a possible entry, because jump targets are computed at
///    runtime and cannot be resolved here.
fn reachable_hits(code: &[u8], targets: &[u8]) -> Vec<(usize, u8)> {
    let Some(start) = exec_start(code) else { return Vec::new() };
    let end = metadata_start(code).unwrap_or(code.len());
    let mut out = Vec::new();
    let mut pc = start;
    let mut live = true;
    while pc < end {
        let op = code[pc];
        if op == 0x5b {
            live = true; // JUMPDEST: a jump may land here
        }
        if live && targets.contains(&op) {
            out.push((pc, op));
        }
        // 0xfc is HEADER only at offset zero; anywhere else it is unassigned,
        // so it halts like any invalid opcode. Only well-known terminators are
        // listed: treating a byte as one *removes* positions, and the targets
        // themselves are valid opcodes below iris300.
        if matches!(op, 0x00 | 0x56 | 0xf3 | 0xfc | 0xfd | 0xfe | 0xff) {
            live = false; // STOP, JUMP, RETURN, HEADER-elsewhere, REVERT, INVALID, SELFDESTRUCT
        }
        // PUSH1..PUSH32 carry their immediate inline. Everything else,
        // including the two-byte DUPN and SWAPN, advances by one: after a hit
        // the decode no longer matters, and stepping by one cannot miss a
        // position a longer step would have reached.
        pc += if (0x60..=0x7f).contains(&op) { 1 + (op - 0x5f) as usize } else { 1 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_inside_a_push_immediate_is_not_an_instruction() {
        // PUSH2 0xa8a9, STOP: the two target bytes are data.
        assert!(reachable_hits(&[0x61, 0xa8, 0xa9, 0x00], &[0xa8, 0xa9]).is_empty());
        // The same bytes as instructions.
        assert_eq!(
            reachable_hits(&[0xa8, 0x00, 0xa9, 0x00], &[0xa8, 0xa9]),
            vec![(0, 0xa8)],
            "0xa9 sits after a STOP with no JUMPDEST between, so it is dead"
        );
    }

    #[test]
    fn a_dead_region_after_a_terminator_is_excluded_until_a_jumpdest() {
        // STOP then data, then JUMPDEST then the real thing.
        let code = [0x00, 0xa8, 0xa9, 0x5b, 0xa8];
        assert_eq!(reachable_hits(&code, &[0xa8, 0xa9]), vec![(4, 0xa8)]);
    }

    #[test]
    fn the_code_header_moves_where_decoding_starts() {
        // Header of one extra byte: execution starts at 5. The 0xa9 inside the
        // header is not an instruction; the one after it is.
        let code = [0xfc, 0x00, 0x00, 0x01, 0xa9, 0xa8, 0x00];
        assert_eq!(exec_start(&code), Some(5));
        assert_eq!(reachable_hits(&code, &[0xa8, 0xa9]), vec![(5, 0xa8)]);

        // The extra length is read unsigned: 0x80 is 128, not -128.
        let mut long = vec![0xfc, 0x81, 0x01, 0x80];
        long.extend([0u8; 128]);
        long.push(0xa8);
        long.push(0x00);
        assert_eq!(exec_start(&long), Some(132));
        assert_eq!(reachable_hits(&long, &[0xa8]), vec![(132, 0xa8)]);
    }

    #[test]
    fn a_degenerate_header_can_execute_nothing() {
        // rskj's testScriptVersion0..3: each fails with all its gas.
        for code in [
            &[0xfc][..],
            &[0xfc, 0x00, 0x00, 0x00][..],
            &[0xfc, 0x00, 0x00, 0x05, 0x00, 0x00][..],
        ] {
            assert_eq!(exec_start(code), None, "{code:?}");
            assert!(reachable_hits(code, &[0xa8, 0xa9, 0xaa]).is_empty());
        }
        // A header followed by a 0xa8 that a naive reader would call reachable:
        // the header reaches the end, so nothing runs.
        assert!(reachable_hits(&[0xfc, 0x00, 0x00, 0x09, 0xa8], &[0xa8]).is_empty());
    }

    #[test]
    fn code_not_starting_with_the_header_byte_decodes_from_zero() {
        assert_eq!(exec_start(&[0x60, 0x01, 0x00]), Some(0));
        assert_eq!(exec_start(&[]), Some(0));
    }

    #[test]
    fn the_metadata_trailer_is_not_code() {
        // STOP, then a 4-byte CBOR-looking trailer holding 0xa9, then its
        // length. Nothing in the trailer is an instruction.
        let code = [0x00, 0xa1, 0x01, 0xa9, 0x02, 0x00, 0x04];
        assert_eq!(metadata_start(&code), Some(1));
        assert!(reachable_hits(&code, &[0xa9]).is_empty());
    }
}

/// The bit path accumulated down to a node, packed MSB-first -- which is the
/// key encoding itself, so a path that lands on a byte boundary *is* the key.
///
/// Packed rather than one byte per bit because every queued item carries a
/// copy: at 256 bits a key, the unpacked form cost 256 bytes per entry and took
/// the walk to 890 MB of resident memory, on a box that also runs the node.
#[derive(Clone, Default)]
struct Path {
    bytes: Vec<u8>,
    bits: usize,
}

impl Path {
    fn push(&mut self, bit: u8) {
        if self.bits.is_multiple_of(8) {
            self.bytes.push(0);
        }
        let i = self.bits / 8;
        self.bytes[i] |= (bit & 1) << (7 - (self.bits % 8));
        self.bits += 1;
    }

    /// The key, when the path is a whole number of bytes. A path that is not
    /// cannot be a leaf key.
    fn as_key(&self) -> Option<&[u8]> {
        self.bits.is_multiple_of(8).then_some(&self.bytes[..])
    }
}

#[derive(Default)]
struct Census {
    /// Hashes already walked.
    ///
    /// Not an optimisation. The trie is content-addressed, so an identical
    /// subtree -- the same bytecode deployed twice, a storage subtree two
    /// contracts share -- is reachable through several parents. Following
    /// every edge re-walks it once per reference, which `trie_tool` records as
    /// having made "no observable progress in 22 minutes" on this chain. The
    /// first version of this tool had the same defect and was stopped for it.
    seen: Mutex<std::collections::HashSet<alloy_primitives::B256>>,
    /// Bytes of node message walked, against the root's own total, for a
    /// percentage. Deduplication means this stops short of 100%: the total
    /// counts a shared subtree once per reference and the walk counts it once.
    bytes_done: AtomicU64,
    nodes: AtomicU64,
    code_leaves: AtomicU64,
    code_bytes: AtomicU64,
    missing: AtomicU64,
    /// Addresses with a reachable target byte, and where.
    hits: Mutex<Vec<(Vec<u8>, usize, u8)>>,
    /// Occurrences anywhere in code, reachable or not. The contrast is the
    /// point: it shows how wrong a plain byte scan would have been.
    present_anywhere: Mutex<HashMap<u8, u64>>,
    /// Contracts carrying the RSK code header, by `(exeVersion, scriptVersion)`
    /// as the header declares them. Its own "was this ever used?" question,
    /// and the answer decides whether this tool's header handling matters at
    /// all on this chain.
    headers: Mutex<HashMap<(u8, u8), Vec<Vec<u8>>>>,
    /// Headered contracts that can execute nothing: a 0xfc too short to be a
    /// header, or one reaching the code's end.
    header_degenerate: AtomicU64,
}

fn main() -> Result<()> {
    let positional: Vec<String> =
        std::env::args().skip(1).take_while(|a| !a.starts_with("--")).collect();
    if positional.len() != 2 {
        eprintln!(
            "usage: opcode_census <block-dir> <trie-dir> [--block N] [--ops a8,a9,aa] \
             [--threads N]"
        );
        std::process::exit(64);
    }
    let block: u64 = flag("block").and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_BLOCK);
    let targets: Vec<u8> = flag("ops")
        .unwrap_or_else(|| "a8,a9,aa".into())
        .split(',')
        .filter_map(|s| u8::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .collect();
    anyhow::ensure!(!targets.is_empty(), "no opcodes to look for");
    let threads: usize = flag("threads")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2));

    let blocks = BlockStore::open_read_only(&positional[0])
        .with_context(|| format!("opening {} read-only", positional[0]))?;
    let hash = blocks
        .canonical_hash(block)?
        .with_context(|| format!("no canonical block at #{block}"))?;
    let header = blocks.header(hash)?.with_context(|| format!("no header at #{block}"))?;
    let root = header.state_root;

    let store: Arc<dyn TrieStore> = Arc::new(
        RocksDbTrieStore::open_read_only(&positional[1])
            .with_context(|| format!("opening {} read-only", positional[1]))?,
    );
    let root_bytes = store.get(root.as_slice()).with_context(|| {
        format!(
            "the state root {root:?} of #{block} is not in {}. An archive reaches only as \
             high as it was built; check with the `archive_bound` tool.",
            positional[1]
        )
    })?;

    let total_bytes = {
        let n = TrieNode::from_message(&root_bytes, store.as_ref());
        n.children_size + n.message_length(store.as_ref()) as u64
    };
    // One address, no walk: a code key is derived from the address, so a
    // candidate can be looked at directly. This is what turns a hit into a
    // judgement -- 443 of the first run's hits were the metadata trailer and
    // constant pools, and the only way to be sure of the survivors is to read
    // the bytes around them.
    if let Some(hex) = flag("dump") {
        let bytes = alloy_primitives::hex::decode(hex.trim_start_matches("0x"))
            .context("--dump takes a hex address")?;
        let mut key = rustock_trie::account_key_from_bytes(&bytes);
        key.push(0x80);
        let node = TrieNode::from_message(&root_bytes, store.as_ref());
        let code = node
            .get(&rustock_trie::TrieKeySlice::from_key(&key), store.as_ref())
            .with_context(|| format!("no code for 0x{} at #{block}", hex.trim_start_matches("0x")))?;
        println!("code for 0x{} at #{block}: {} bytes", hex.trim_start_matches("0x"), code.len());
        println!("exec starts at {:?}, metadata at {:?}", exec_start(&code), metadata_start(&code));
        let at: Option<usize> = flag("at").and_then(|v| v.parse().ok());
        let (from, to) = match at {
            Some(a) => (a.saturating_sub(24), (a + 24).min(code.len())),
            None => (0, code.len().min(64)),
        };
        print!("bytes {from}..{to}:");
        for (i, b) in code[from..to].iter().enumerate() {
            if Some(from + i) == at {
                print!(" [{b:02x}]");
            } else {
                print!(" {b:02x}");
            }
        }
        println!();
        for (pc, op) in reachable_hits(&code, &targets) {
            println!("  reachable 0x{op:02x} at {pc}");
        }
        return Ok(());
    }

    println!("scanning the state at #{block} ({root:?})");
    println!("the root reports {} MB of trie beneath it", total_bytes / (1 << 20));
    println!(
        "looking for {} at positions a linear decode calls instructions, on {threads} thread(s)",
        targets.iter().map(|o| format!("0x{o:02x}")).collect::<Vec<_>>().join(" ")
    );

    let census = Arc::new(Census::default());
    // Work-stealing over subtrees rather than a pre-computed shard split: the
    // trie is deeply uneven, and the cost here is random reads, so idle
    // workers matter more than lock contention on a stack touched once per
    // node.
    let work: Arc<Mutex<Vec<(B256Like, Path)>>> = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();

    // Seed with the root, already in hand.
    {
        let node = TrieNode::from_message(&root_bytes, store.as_ref());
        let mut w = work.lock().unwrap();
        push_node(&node, Path::default(), &mut w, &census, &targets, store.as_ref());
    }

    let idle = AtomicU64::new(0);
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    std::thread::scope(|scope| {
        // Progress, because this is a walk of millions of nodes against a
        // 130 GB store and silence for an hour is indistinguishable from a
        // hang. Queue depth is included: it is what says whether the walk is
        // still finding work or draining.
        {
            let census = census.clone();
            let work = work.clone();
            let done = done.clone();
            scope.spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    let n = census.nodes.load(Ordering::Relaxed);
                    let secs = started.elapsed().as_secs_f64().max(0.001);
                    let done_bytes = census.bytes_done.load(Ordering::Relaxed);
                    let pct = done_bytes as f64 * 100.0 / total_bytes.max(1) as f64;
                    let eta = if pct > 0.5 {
                        format!("{:.0} min left", (secs / (pct / 100.0) - secs) / 60.0)
                    } else {
                        "eta unknown".into()
                    };
                    eprintln!(
                        "  {n} nodes ({:.0}/s), {pct:.1}% by bytes, {eta}, {} contracts, \
                         {} MB code, {} missing, queue {}, seen {}",
                        n as f64 / secs,
                        census.code_leaves.load(Ordering::Relaxed),
                        census.code_bytes.load(Ordering::Relaxed) / (1 << 20),
                        census.missing.load(Ordering::Relaxed),
                        work.lock().unwrap().len(),
                        census.seen.lock().unwrap().len()
                    );
                }
            });
        }
        // Workers in their own scope, which joins here. `done` was previously
        // set straight after the spawns -- `scope.spawn` returns immediately,
        // so the reporter saw it on its first check and the walk ran blind.
        std::thread::scope(|inner| {
            for _ in 0..threads {
                let work = work.clone();
                let census = census.clone();
                let store = store.clone();
                let targets = targets.clone();
                let idle = &idle;
                inner.spawn(move || worker(&work, &census, store.as_ref(), &targets, idle, threads));
            }
        });
        done.store(true, Ordering::Relaxed);
    });

    let secs = started.elapsed().as_secs_f64();
    let nodes = census.nodes.load(Ordering::Relaxed);
    let code_leaves = census.code_leaves.load(Ordering::Relaxed);
    let code_bytes = census.code_bytes.load(Ordering::Relaxed);
    let missing = census.missing.load(Ordering::Relaxed);
    let hits = census.hits.lock().unwrap();
    let anywhere = census.present_anywhere.lock().unwrap();

    println!();
    println!("nodes walked      {nodes}");
    println!("contracts         {code_leaves}");
    println!("code bytes        {code_bytes} ({} MB)", code_bytes / (1 << 20));
    println!("elapsed           {secs:.0}s");
    println!();
    {
        let headers = census.headers.lock().unwrap();
        let total: usize = headers.values().map(|v| v.len()).sum();
        let degenerate = census.header_degenerate.load(Ordering::Relaxed);
        if total == 0 && degenerate == 0 {
            println!(
                "RSK code header (0xfc): never used. No contract alive at #{block} carries \
                 one, so the header's rules decide nothing on this chain."
            );
        } else {
            println!("RSK code header (0xfc): {total} contract(s), {degenerate} degenerate");
            for ((exe, script), addrs) in headers.iter() {
                println!(
                    "  exeVersion {exe}, scriptVersion {script}: {} contract(s), e.g. 0x{}",
                    addrs.len(),
                    addrs[0].iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
            }
        }
    }
    println!();
    println!("occurrences ANYWHERE in code (what a byte scan would report):");
    for op in &targets {
        println!("  0x{op:02x}  {}", anywhere.get(op).copied().unwrap_or(0));
    }
    println!();
    println!("occurrences AT AN INSTRUCTION POSITION (what can execute):");
    let mut by_op: HashMap<u8, u64> = HashMap::new();
    for (_, _, op) in hits.iter() {
        *by_op.entry(*op).or_default() += 1;
    }
    for op in &targets {
        println!("  0x{op:02x}  {}", by_op.get(op).copied().unwrap_or(0));
    }

    if !hits.is_empty() {
        println!();
        println!("candidates (address, offset, opcode):");
        for (addr, pc, op) in hits.iter().take(200) {
            println!(
                "  0x{}  pc={pc}  0x{op:02x}",
                addr.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
        }
        if hits.len() > 200 {
            println!("  ... and {} more", hits.len() - 200);
        }
    }

    println!();
    if missing > 0 {
        println!(
            "INCOMPLETE: {missing} reference(s) could not be resolved, so the walk did not \
             cover the whole state and a clean result would mean nothing -- an absent \
             subtree is exactly where the answer could hide. Point --trie-dir at a store \
             that holds this state in full."
        );
        std::process::exit(2);
    }
    if hits.is_empty() {
        println!(
            "NONE REACHABLE: no contract alive at #{block} can execute any of these bytes. \
             Still outstanding: contracts created and destroyed before #{block}, whose code \
             is in no state -- a scan of transaction input data is the second pass for that."
        );
    } else {
        println!(
            "{} reachable occurrence(s) in {} contract(s). Each is a contract that *could* \
             execute one, not proof that any transaction did; re-execute the blocks touching \
             these addresses to settle it.",
            hits.len(),
            hits.iter().map(|(a, _, _)| a).collect::<std::collections::HashSet<_>>().len()
        );
    }
    Ok(())
}

/// Pop a subtree, read it, account for it, queue its children.
///
/// Termination: a worker that finds the stack empty announces itself idle and
/// waits; when every worker is idle at once there is no outstanding node that
/// could queue more, so the walk is over.
fn worker(
    work: &Mutex<Vec<(B256Like, Path)>>,
    census: &Census,
    store: &dyn TrieStore,
    targets: &[u8],
    idle: &AtomicU64,
    threads: usize,
) {
    loop {
        let item = work.lock().unwrap().pop();
        let Some((hash, bits)) = item else {
            if idle.fetch_add(1, Ordering::SeqCst) as usize + 1 >= threads {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            idle.fetch_sub(1, Ordering::SeqCst);
            if work.lock().unwrap().is_empty() {
                return;
            }
            continue;
        };
        let Some(data) = store.get(hash.0.as_slice()) else {
            census.missing.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        let node = TrieNode::from_message(&data, store);
        let mut w = work.lock().unwrap();
        push_node(&node, bits, &mut w, census, targets, store);
    }
}

/// Newtype so the work stack holds something `Send` without pulling alloy into
/// the signature everywhere.
struct B256Like(alloy_primitives::B256);

/// Account for a node: record its value if it is code, and queue its children.
fn push_node(
    node: &TrieNode,
    prefix: Path,
    work: &mut Vec<(B256Like, Path)>,
    census: &Census,
    targets: &[u8],
    store: &dyn TrieStore,
) {
    census.nodes.fetch_add(1, Ordering::Relaxed);

    let mut here = prefix;
    for i in 0..node.shared_path.length() {
        here.push(node.shared_path.get(i));
    }

    let has_value = node.value.is_some() || node.value_hash.is_some();
    if has_value {
        if let Some(key) = here.as_key() {
            if let Some(addr) = code_address(key) {
                let code = match (&node.value, &node.value_hash) {
                    (Some(v), _) => Some(v.clone()),
                    (None, Some(h)) => store.get(h.as_slice()),
                    _ => None,
                };
                match code {
                    Some(code) => {
                        census.code_leaves.fetch_add(1, Ordering::Relaxed);
                        census.code_bytes.fetch_add(code.len() as u64, Ordering::Relaxed);
                        {
                            let mut anywhere = census.present_anywhere.lock().unwrap();
                            for b in &code {
                                if targets.contains(b) {
                                    *anywhere.entry(*b).or_default() += 1;
                                }
                            }
                        }
                        if code.first() == Some(&0xfc) {
                            if code.len() >= 4 {
                                census
                                    .headers
                                    .lock()
                                    .unwrap()
                                    .entry((code[1], code[2]))
                                    .or_default()
                                    .push(addr.clone());
                            }
                            if exec_start(&code).is_none() {
                                census.header_degenerate.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        let hits = reachable_hits(&code, targets);
                        if !hits.is_empty() {
                            let mut h = census.hits.lock().unwrap();
                            for (pc, op) in hits {
                                h.push((addr.clone(), pc, op));
                            }
                        }
                    }
                    // A code leaf whose long value is absent: the one thing
                    // that could hide an answer, so it counts as missing.
                    None => {
                        census.missing.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    census.bytes_done.fetch_add(node.message_length(store) as u64, Ordering::Relaxed);

    for (bit, child) in [(0u8, &node.left), (1u8, &node.right)] {
        match child {
            NodeRef::Empty => {}
            NodeRef::Hash(h) => {
                if !census.seen.lock().unwrap().insert(*h) {
                    continue;
                }
                let mut b = here.clone();
                b.push(bit);
                work.push((B256Like(*h), b));
            }
            // Embedded in the parent's message: already in hand, no read.
            NodeRef::Node(n) => {
                let mut b = here.clone();
                b.push(bit);
                push_node(n, b, work, census, targets, store);
            }
        }
    }
}
