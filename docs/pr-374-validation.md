# What #374 was tested against, and what it was not

[#374](https://github.com/Rustock-dev/Rustock/pull/374) — 343 files,
+112,236/−13,597, 264 commits of RSKj parity work from an external
contributor — was merged into `main` as `b381da56` on 2026-11-18 with CI green
except one check, on the repository owner's explicit authorization.

This records the evidence behind that, so nobody has to reconstruct it. **It is
not a clearance.** 165 of the 343 changed files are in `crates/execution` and
the consensus logic was never line-reviewed.

## Supply-chain screen

Read through the GitHub API, never checked out, against the three questions in
`CLAUDE.md`:

| question | result |
|---|---|
| New `[[package]]` in `Cargo.lock`? | **clean** — 3 added lines, all dependency *edges* to already-locked crates (`sha2`, `num-bigint` ×2); zero new stanzas |
| `build.rs` / `.cargo/config.toml` / `rust-toolchain.toml` / proc-macro? | **clean** — none |
| Non-Rust executable? | **one** — `.github/workflows/ci.yml`, adding a macOS/Windows matrix with pinned standard actions, no secrets, no untrusted interpolation |

`deny.toml` is modified but comment-only: no new allows, no advisory
suppressions. New outbound endpoints (`import.mainnet.rskcomputing.net`,
`rsk.co/price`) are both opt-in — `[mining.stable_gas_price]` defaults to
`enabled = false` and is a *mining* feature the production node does not use;
`--import` is an explicit flag.

One screening note worth keeping: `gh pr view --json files` silently returns
**100 of 343 files**. Paginate, or screen a truncated list without knowing it.

## Tests run, all in a bwrap sandbox

No network, no credentials, `/srv/rustock` and the store hidden except where
bound read-only. Store write is independently ACL-denied. Mainnet node stopped
throughout.

| check | result |
|---|---|
| Release build, `--features smtp` (production config) | clean, 5m18s |
| Full workspace test suite | **4,110 passed, 0 failed**, 8 ignored |
| `mainnet_block_rules --ignored` vs real history | **4 tests, 0 failures**, 241s |
| Header re-hash, 17 activation boundaries | **~803,000 headers, 0 mismatches** |
| State-root re-execution, **whole Unitrie era** | **7,638,848 blocks, every root matched** |

### Block rules

```
fork-detection blocks checked: 300, failures 0
RSKIP179: 672 headers (281 below iris300), largest gap 276s vs 300s bound, failures 0
checked 428, skipped 0, failures 0
uncle-bearing blocks checked: 12531, failures 0, skipped with ancestry pruned 0
```

That last `0` settles [#386](https://github.com/Rustock-dev/Rustock/issues/386)
for this store: it is unpruned, so the silent-skip path never fires and the
pass is not hollow. **The harness samples**, though — 428 blocks for the
general rules out of 9.42M. Only the uncle test is broad.

### Header decoding

Every mainnet activation height ±25,000 blocks, plus `bahamas`,
`afterBridgeSync`, `hop401`, `rskip351` and the head region. Each stored header
decoded under the new strict decoder and re-hashed to the key it is filed
under; zero `Rehashed` records. Covers canonical chain plus freezer — **stored
non-canonical and uncle headers in `CF_HEADERS` are not covered**
([#376](https://github.com/Rustock-dev/Rustock/issues/376)).

### State-root re-execution: the whole Unitrie era

```
7,638,848 blocks re-executed in 11.3 h (187.3 blk/s aggregate)
segment gaps: 568,032 reads served by the fallback, 0 absent in both
ALL SEGMENTS VERIFIED
```

Every block from **#1,591,001 to #9,230,008** — 95 segments, the complete
Unitrie era up to segbuild's frontier — re-executed with its computed state
root compared against the header's. Zero mismatches.

This is the check #374 lacked when it was merged, and it passes.

**It required fixing the instrument first.** An initial run against segbuild
alone diverged at #8,922,011 and #9,021,501, which looked like a consensus bug
in #374: `replay_bench` reproduced it, pre-374 matched the header, and
`git bisect` named the commit. All of it was wrong. The segments were missing
RSKIP379 pegout-index nodes that #374 reads and earlier code never did, so
`TrieStore::get` returned `None` where the chain has a value. Against the
complete 131 GB archival trie both blocks match exactly. See
`trie-segments-design.md` §6.9d, and #397/#398.

The lesson is worth keeping: **a passing run of the previous version is not a
control** when the newer version reads state the older one never touched. The
failure mode is silent and only ever appears in the newer code.

Nineteen blocks were affected in total; the segments were repaired (56,709
nodes, #397) and verified segment-only afterwards.

### The earlier scoped pass

The first *state-level* check: transactions re-executed, computed roots
compared against headers. Fourteen 2,001-block ranges centred on every
activation from `twoToThree` (#2,018,000) up, including every Bridge boundary
— `hop401` (RSKIP353+357), `fingerroot500` (RSKIP374+385), `arrowhead600`
(RSKIP376+400), `reed800` (RSKIP305), `lovell700` (RSKIP419+438). Read from
segbuild staged into `tmpfs`; see `trie-segments-design.md` §6.9c.

## Performance: no regression from #374

Interleaved A/B, median of three, `16fa0707` (pre) vs `b381da56` (post) — the
two merge parents, differing by exactly #374:

| | median |
|---|---|
| PRE `16fa0707` | 4.36 blocks/s |
| POST `b381da56` | 4.50 blocks/s |

Ratio **1.032**, and POST led in all three interleaved pairs. No material
change.

## What is NOT covered

- **Blocks 1–1,590,999 — 17% of the chain.** No Unitrie pre-state exists to
  seed a mid-chain worker, so re-execution cannot verify them until the
  recorded-roots baseline of `trie-segments-design.md` §8 is built. Includes
  `bahamas` and `orchid`. Header decoding *is* covered there (28,397 headers,
  all fallback-mined signatures checked).
- **Coverage is thin by design.** 28,014 re-executed blocks of 9.23M is 0.3%;
  803k headers of 9.42M is 8.5%. Targeted at where divergence is likeliest,
  not comprehensive.
- **macOS: never ran.** The job was cancelled twice at exactly 901 s having
  been assigned no runner (`runner_name` empty, 0 steps). Not a test failure —
  a queue timeout that `gh pr checks` renders identically to one
  ([#390](https://github.com/Rustock-dev/Rustock/issues/390)). The PR's own
  code did pass macOS at `59d8cd41` before `main` was merged in.
- **Open findings a passing replay cannot answer:**
  [#377](https://github.com/Rustock-dev/Rustock/issues/377) (indexes not
  crash-safe across a branch switch) and
  [#380](https://github.com/Rustock-dev/Rustock/issues/380) (which writes
  survive a throwing Bridge call — a possible consensus divergence). A throwing
  Bridge call must *occur* in a sampled range to be exercised, and that was
  never established.
- **Three regression tests were deleted**, including the #9,251,058 stall that
  wedged this node for three days
  ([#379](https://github.com/Rustock-dev/Rustock/issues/379)).

## Not deployed

As of this record the binary is built and tested but **not installed**. The
node remains stopped. Deploying is a separate decision under the `CLAUDE.md`
authorization gate.

Rollback caveat worth stating: if a diverged state is written before anyone
notices, restoring the previous binary may not be enough and a resync could be
needed. Back up `/usr/local/bin/rustock` before installing.
