//! A contract's bytecode, and the function selectors it dispatches on.
//!
//! The state tools answer whether an account exists and what it holds;
//! none of them answer *what it is*. When a measurement points at an address --
//! a high-water mark, a census hit, a divergence -- the next question is always
//! what the code does, and that needs the code.
//!
//! ```text
//! contract_code <data-dir> <trie-dir> <0xaddress>...
//!               [--block N] [--engine flatdb|rocksdb] [--plain] [--hex]
//! ```
//!
//! `--plain` opens the trie directory as a single rustock-format store rather
//! than an epoch store, which is what an archival trie and a segbuild segment
//! are. Without it the directory is opened as an epoch store, which is what the
//! node keeps.
//!
//! # What the selector list is, and is not
//!
//! Solidity dispatches by comparing the first four octets of calldata against
//! `PUSH4` constants, so the `PUSH4` immediates in a contract's code are its
//! likely entry points. The list is a strong hint and not a proof: a `PUSH4`
//! can be any constant, a dispatcher can be written without one, and a proxy
//! has none of the selectors it actually serves.
//!
//! The decode is linear and honours `PUSH` immediates, so an octet *inside* a
//! push is never read as an opcode -- the same discipline `opcode_census` uses,
//! and the reason a naive byte scan over-reports. It also skips RSK's `0xFC`
//! code header, which moves the offset every instruction decodes from.
//!
//! Read-only, and safe to run beside a live node.

use anyhow::{Context, Result};
use rustock_trie::{code_key_from_bytes, TrieKeySlice, TrieNode, TrieStore};
use std::sync::Arc;

fn flag(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.windows(2).find(|w| w[0] == format!("--{name}")).map(|w| w[1].clone())
}
fn has(name: &str) -> bool {
    std::env::args().any(|a| a == format!("--{name}"))
}

/// `(selectors, instructions)` from a linear decode that honours `PUSH`.
fn decode(code: &[u8]) -> (Vec<[u8; 4]>, usize) {
    // RSK's code header: `0xFC`, then three octets, the third giving how many
    // more follow. Execution and jumpdest analysis both start after it.
    let mut i = if code.first() == Some(&0xfc) && code.len() >= 4 {
        4 + code[3] as usize
    } else {
        0
    };
    let (mut sels, mut n) = (Vec::new(), 0usize);
    while i < code.len() {
        let op = code[i];
        n += 1;
        if (0x60..=0x7f).contains(&op) {
            let width = (op - 0x5f) as usize;
            if op == 0x63 && i + 4 < code.len() {
                sels.push([code[i + 1], code[i + 2], code[i + 3], code[i + 4]]);
            }
            i += 1 + width;
        } else {
            i += 1;
        }
    }
    sels.sort_unstable();
    sels.dedup();
    (sels, n)
}

/// `--sig "token0()"` prints that signature's selector and exits.
///
/// The point is to turn a guess into a check. A selector list identifies a
/// contract only if the names behind it are verified, and recalling that
/// `0x0dfe1681` is `token0()` is exactly the kind of claim that should be
/// computed rather than remembered.
fn selectors_from_signatures() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let sigs: Vec<&String> = args
        .windows(2)
        .filter(|w| w[0] == "--sig")
        .map(|w| &w[1])
        .collect();
    if sigs.is_empty() {
        return false;
    }
    for sig in sigs {
        let h = alloy_primitives::keccak256(sig.as_bytes());
        println!("0x{}  {sig}", alloy_primitives::hex::encode(&h[0..4]));
    }
    true
}

fn main() -> Result<()> {
    if selectors_from_signatures() {
        return Ok(());
    }
    let pos: Vec<String> =
        std::env::args().skip(1).take_while(|a| !a.starts_with("--")).collect();
    if pos.len() < 3 {
        eprintln!(
            "usage: contract_code <data-dir> <trie-dir> <0xaddress>... \
             [--block N] [--engine flatdb|rocksdb] [--plain] [--hex]"
        );
        std::process::exit(64);
    }
    let (data_dir, trie_dir) = (pos[0].clone(), pos[1].clone());

    let blocks = rustock_storage::BlockStore::open_read_only(&data_dir)
        .with_context(|| format!("opening {data_dir} read-only"))?;
    let (at, root) = match flag("block").and_then(|v| v.parse::<u64>().ok()) {
        Some(n) => {
            let h = blocks.canonical_hash(n)?.with_context(|| format!("no block #{n}"))?;
            (n, blocks.header(h)?.with_context(|| format!("no header #{n}"))?.state_root)
        }
        None => {
            let (h, r) = blocks.exec_head()?.context("this node has executed nothing")?;
            (blocks.header(h)?.map(|x| x.number).unwrap_or(0), r)
        }
    };

    let store: Arc<dyn TrieStore> = if has("plain") {
        Arc::new(
            rustock_storage::RocksDbTrieStore::open_read_only(&trie_dir)
                .with_context(|| format!("opening {trie_dir} as a plain rustock store"))?,
        )
    } else {
        let engine = flag("engine").unwrap_or_else(|| "flatdb".into());
        let cfg = rustock_storage::epoch_store::EpochConfig::default();
        let expected = rustock_storage::epoch_backend::keys_for_rotation(cfg.rotate_bytes);
        Arc::new(
            rustock_storage::epoch_store::EpochTrieStore::open_read_only_with(
                std::path::Path::new(&trie_dir),
                cfg,
                rustock_storage::TrieEngine::parse(&engine)?.factory(expected),
            )
            .with_context(|| format!("opening {trie_dir} as an epoch store ({engine})"))?,
        )
    };

    let root_bytes = store.get(root.as_slice()).with_context(|| {
        format!("the state root of #{at} is not in {trie_dir}; a collected store answers only \
                 above its watermark, and an archive only to the height it was built to")
    })?;
    let node = TrieNode::from_message(&root_bytes, store.as_ref());
    println!("state at #{at}\n");

    for raw in &pos[2..] {
        let hex = raw.trim().trim_start_matches("0x");
        let bytes = alloy_primitives::hex::decode(hex)
            .with_context(|| format!("{raw} is not a hex address"))?;
        let key = code_key_from_bytes(&bytes);
        match node.get(&TrieKeySlice::from_key(&key), store.as_ref()) {
            None => println!("0x{hex}\n  NO CODE -- an EOA, or a contract with none\n"),
            Some(code) => {
                let (sels, insns) = decode(&code);
                println!("0x{hex}");
                println!("  code      {} octets, {insns} instructions", code.len());
                if code.first() == Some(&0xfc) {
                    println!("  header    RSK 0xFC code header present");
                }
                println!("  PUSH4 constants ({}), likely selectors:", sels.len());
                for chunk in sels.chunks(8) {
                    let line: Vec<String> =
                        chunk.iter().map(|s| format!("0x{}", alloy_primitives::hex::encode(s))).collect();
                    println!("    {}", line.join(" "));
                }
                if has("hex") {
                    println!("  {}", alloy_primitives::hex::encode(&code));
                }
                println!();
            }
        }
    }
    Ok(())
}
