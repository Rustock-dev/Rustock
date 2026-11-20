//! The pre-RSKIP126 root baseline: one computed Unitrie root per block for
//! blocks 1..=1,591,000.
//!
//! Headers below #1,591,000 declare Orchid-format roots, so they cannot be
//! compared against a Unitrie root and cannot seed a Unitrie replay. This file
//! supplies both: the expected root for every block of the era, and -- because
//! a worker can seek to any block in constant time -- the seed for a range
//! starting anywhere inside it. That is what lets the era be split into
//! segments and replayed in parallel like the rest of the chain
//! (`docs/trie-segments-design.md` §8, §8.2).
//!
//! **What a match against this file does and does not prove.** The roots were
//! recorded by rustock, so agreeing with them is regression detection, not
//! correctness: a wrong root at #400,000 would be recorded as readily as a
//! right one and every later run would agree with it. The era has exactly one
//! independent check -- #1,591,000 is the first block whose header carries a
//! Unitrie root, so matching it validates the whole cumulative pre-fork state.
//! The anchor is stored in the header here and `open` refuses a file whose
//! anchor disagrees with its own last root, but only a caller comparing the
//! anchor against the block store closes the loop. `anchor()` exists to be
//! used, and a verifier that skips it has checked rustock against itself.
//!
//! Layout, little-endian, fixed 32-byte stride so block *N* sits at
//! `HEADER_LEN + (N - first) * 32` with no index:
//!
//! ```text
//!   0   8   magic "RSKBASE1"
//!   8   4   schema version
//!  12   4   reserved, zero
//!  16   8   first block
//!  24   8   last block
//!  32  32   anchor: the root at `last`
//!  64   ..  roots, 32 bytes each
//! ```
#![allow(dead_code)] // Each example uses a different subset.

use anyhow::{bail, ensure, Context, Result};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"RSKBASE1";
/// Bump when the meaning of a recorded root changes -- a different trie
/// encoding or key mapping legitimately changes every root in the file. Without
/// this, such a change reads as 1,591,000 verification failures instead of one
/// "the baseline is stale" (§8).
pub const SCHEMA: u32 = 1;
pub const HEADER_LEN: usize = 64;
pub const ROOT_LEN: usize = 32;

/// First block whose header carries a Unitrie root, and so the anchor and the
/// last block this file covers.
pub const UNITRIE_FROM: u64 = 1_591_000;

pub struct Baseline {
    bytes: Vec<u8>,
    first: u64,
    last: u64,
    anchor: [u8; 32],
}

impl Baseline {
    /// Read and validate a baseline file. ~49 MB, read whole: at 32 bytes a
    /// block, seeking per block would cost 1.59M syscalls to save nothing.
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading baseline {}", path.display()))?;
        ensure!(
            bytes.len() >= HEADER_LEN,
            "{} is {} bytes, too short to be a baseline",
            path.display(),
            bytes.len()
        );
        if &bytes[0..8] != MAGIC {
            bail!(
                "{} starts with {:?}, not {:?}; this is not a baseline file",
                path.display(),
                String::from_utf8_lossy(&bytes[0..8]),
                String::from_utf8_lossy(MAGIC),
            );
        }
        let schema = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if schema != SCHEMA {
            bail!(
                "{} has baseline schema {schema}, this build expects {SCHEMA}. The recorded \
                 roots were produced under different trie semantics, so every one of them \
                 would now mismatch. Regenerate the baseline rather than trusting it.",
                path.display()
            );
        }
        let first = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let last = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        ensure!(first >= 1 && last >= first, "baseline range #{first}..#{last} is not sensible");
        let anchor: [u8; 32] = bytes[32..64].try_into().unwrap();

        let count = (last - first + 1) as usize;
        let want = HEADER_LEN + count * ROOT_LEN;
        ensure!(
            bytes.len() == want,
            "{} is {} bytes but its range #{first}..#{last} needs {want}; truncated or corrupt",
            path.display(),
            bytes.len()
        );

        let me = Baseline { bytes, first, last, anchor };

        // Self-consistency only: proves the header was not edited away from the
        // body. It says nothing about whether the roots are right -- see the
        // module note on what closes that loop.
        let tail = me.root(last).expect("in range");
        ensure!(
            tail == anchor,
            "baseline header anchor {} disagrees with its own root for #{last} ({})",
            hex(&anchor),
            hex(&tail)
        );
        Ok(me)
    }

    pub fn first(&self) -> u64 {
        self.first
    }

    pub fn last(&self) -> u64 {
        self.last
    }

    /// The root at the last covered block, which for mainnet is #1,591,000 and
    /// is the era's only check against the chain rather than against rustock.
    pub fn anchor(&self) -> [u8; 32] {
        self.anchor
    }

    pub fn covers(&self, block: u64) -> bool {
        block >= self.first && block <= self.last
    }

    /// The recorded root for `block`, or `None` if outside the covered range.
    /// `None` is deliberately not "absent": the file is dense by construction,
    /// so out-of-range is the only way to miss, and a caller must not read a
    /// miss as permission to skip verifying the block.
    pub fn root(&self, block: u64) -> Option<[u8; 32]> {
        if !self.covers(block) {
            return None;
        }
        let off = HEADER_LEN + ((block - self.first) as usize) * ROOT_LEN;
        Some(self.bytes[off..off + ROOT_LEN].try_into().unwrap())
    }

    pub fn len(&self) -> u64 {
        self.last - self.first + 1
    }
}

/// Write a baseline from roots in ascending block order.
///
/// `anchor` is checked against the last root rather than taken on trust, so a
/// caller that passes the block store's header root for `last` cannot produce
/// a file whose anchor is a wish.
pub fn write(
    path: &Path,
    first: u64,
    last: u64,
    roots: &[[u8; 32]],
    anchor: [u8; 32],
) -> Result<()> {
    ensure!(last >= first, "range #{first}..#{last} is not sensible");
    let count = (last - first + 1) as usize;
    ensure!(
        roots.len() == count,
        "{} roots given for range #{first}..#{last}, which needs {count}",
        roots.len()
    );
    ensure!(
        roots[count - 1] == anchor,
        "the anchor {} is not the root given for #{last} ({})",
        hex(&anchor),
        hex(&roots[count - 1])
    );

    let mut out = Vec::with_capacity(HEADER_LEN + count * ROOT_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SCHEMA.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&first.to_le_bytes());
    out.extend_from_slice(&last.to_le_bytes());
    out.extend_from_slice(&anchor);
    for r in roots {
        out.extend_from_slice(r);
    }
    debug_assert_eq!(out.len(), HEADER_LEN + count * ROOT_LEN);

    // Write beside the target and rename, so an interrupted write cannot leave
    // a half file that `open` would have to distinguish from a real one.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &out).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + b.len() * 2);
    s.push_str("0x");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}
