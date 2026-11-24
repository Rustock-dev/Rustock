//! An account's nonce and balance, as the trie holds them at a chosen block.
//!
//! One lookup, no walk: a unitrie account key is derived from the address, so
//! the state root plus the address is enough. Nothing here executes anything.
//!
//! ```text
//! account_at <data-dir> <trie-dir> <0xaddress>…
//!            [--block N] [--engine flatdb|rocksdb]
//! ```
//!
//! `--block` defaults to the node's **executed** head, which is the state a
//! balance ordinarily means; pass a height for a historical one, which needs a
//! store that still holds that state (`archive_bound` says how high an archive
//! reaches, and a garbage-collected store answers only above its watermark --
//! `segbuild_status` prints that).
//!
//! `--engine` defaults to `flatdb`, which is what the mainnet node runs; the
//! archive is `rocksdb`. Passing the wrong one fails at the open rather than
//! quietly answering from nothing.
//!
//! An absent account is reported as absent rather than as a zero balance. The
//! two are different facts, and on this chain the distinction carries weight:
//! a precompile address exists as an account from an activation onward whatever
//! its balance, and an empty account is kept rather than pruned (`BR-057`).
//!
//! Read-only, and safe to run against a live node.

use alloy_primitives::U256;
use anyhow::{Context, Result};
use rustock_storage::epoch_store::{EpochConfig, EpochTrieStore};
use rustock_storage::BlockStore;
use rustock_trie::{account_key_from_bytes, AccountState, TrieKeySlice, TrieNode, TrieStore};
use std::sync::Arc;

fn flag(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|w| w[0] == format!("--{name}")).map(|w| w[1].clone())
}

/// Ten-to-the-eighteenth, so a balance reads as RBTC as well as in weis.
fn as_rbtc(wei: U256) -> String {
    let unit = U256::from(10u64).pow(U256::from(18));
    let whole = wei / unit;
    let frac = wei % unit;
    // Eighteen places, trimmed, so a dust balance is not rounded to nothing.
    let frac = format!("{:018}", frac).trim_end_matches('0').to_string();
    if frac.is_empty() {
        format!("{whole}")
    } else {
        format!("{whole}.{frac}")
    }
}

fn main() -> Result<()> {
    let positional: Vec<String> =
        std::env::args().skip(1).take_while(|a| !a.starts_with("--")).collect();
    if positional.len() < 3 {
        eprintln!(
            "usage: account_at <data-dir> <trie-dir> <0xaddress>... \
             [--block N] [--engine flatdb|rocksdb]"
        );
        std::process::exit(64);
    }
    let (data_dir, trie_dir) = (positional[0].clone(), positional[1].clone());
    let engine = flag("engine").unwrap_or_else(|| "flatdb".into());

    let blocks = BlockStore::open_read_only(&data_dir)
        .with_context(|| format!("opening {data_dir} read-only"))?;

    // The executed head, not the best header: only executed blocks have state.
    let (at, root) = match flag("block").and_then(|v| v.parse::<u64>().ok()) {
        Some(number) => {
            let hash = blocks
                .canonical_hash(number)?
                .with_context(|| format!("no canonical block at #{number}"))?;
            let header =
                blocks.header(hash)?.with_context(|| format!("no header at #{number}"))?;
            (number, header.state_root)
        }
        None => {
            let (hash, root) = blocks.exec_head()?.context("this node has executed nothing")?;
            let number = blocks.header(hash)?.map(|h| h.number).unwrap_or(0);
            (number, root)
        }
    };

    let engine_parsed = rustock_storage::TrieEngine::parse(&engine)?;
    let config = EpochConfig::default();
    let expected = rustock_storage::epoch_backend::keys_for_rotation(config.rotate_bytes);
    let store: Arc<dyn TrieStore> = Arc::new(
        EpochTrieStore::open_read_only_with(
            std::path::Path::new(&trie_dir),
            config,
            engine_parsed.factory(expected),
        )
        .with_context(|| format!("opening {trie_dir} read-only as {engine}"))?,
    );

    let root_bytes = store.get(root.as_slice()).with_context(|| {
        format!(
            "the state root {root:?} of #{at} is not in {trie_dir}. A garbage-collected \
             store answers only above its collection watermark, and an archive only up to \
             the height it was built to."
        )
    })?;
    let node = TrieNode::from_message(&root_bytes, store.as_ref());

    println!("state at #{at}  root {root:?}");
    println!("store {trie_dir} ({engine})");
    println!();

    for raw in &positional[2..] {
        let hex = raw.trim().trim_start_matches("0x");
        let bytes = alloy_primitives::hex::decode(hex)
            .with_context(|| format!("{raw} is not a hex address"))?;
        anyhow::ensure!(
            bytes.len() == 20 || bytes.len() == 1,
            "{raw} is {} octets; an address is 20 (or 1, for REMASC)",
            bytes.len()
        );
        let key = account_key_from_bytes(&bytes);
        match node.get(&TrieKeySlice::from_key(&key), store.as_ref()) {
            Some(encoded) => {
                let account = AccountState::decode(&encoded)
                    .with_context(|| format!("decoding the account state of 0x{hex}"))?;
                println!("0x{hex}");
                println!("  nonce    {}", account.nonce);
                println!("  balance  {} wei", account.balance);
                println!("           {} RBTC", as_rbtc(account.balance));
                if account.state_flags != 0 {
                    println!("  flags    0x{:08x}", account.state_flags);
                }
            }
            // Absent is not zero. See the module note.
            None => println!("0x{hex}\n  ABSENT -- no account at this address in this state"),
        }
    }
    Ok(())
}
