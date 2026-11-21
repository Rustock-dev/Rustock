# Generating a segbuild set from scratch

The segment set and the root baseline are data, not code, so a checkout does not
contain them. This is how to produce them, and what the one irreplaceable input
is.

For what a segbuild set *is* and how to read one, see
[`reexecution-rustock-vs-rskj.md`](reexecution-rustock-vs-rskj.md) §1–§3. For
why it is built the way it is, [`trie-segments-design.md`](trie-segments-design.md).
This document is the procedure.

---

## 0. What you need before starting

| input | size | obtainable? |
|---|---|---|
| a block store | — | yes: sync a node |
| an archive trie | ~131 GB | yes: sync with pruning off, or an archive snapshot |
| recorded pre-fork roots **or** rskj's `stateRoots` | 135 MB / 119 MB | the first yes, the second **not regenerable** |

**The one scarce input.** rskj's `stateRoots` table maps each pre-RSKIP126
Orchid state root to the Unitrie root holding the same state. It can only come
from a mainnet snapshot taken from a node that **executed** blocks
#1..#1,591,000 — a full sync from genesis, not a snap sync, which never builds
the table at all. Orchid roots exist only below the fork, so no node will ever
add an entry.

You do not strictly need it: the baseline can be built from roots rustock
records itself (§2.1). But then the pre-fork era has only one independent check
— the anchor at #1,591,000 — instead of a per-block oracle, and verifying a
baseline against roots rustock produced is circular. If you can get the table,
get it.

---

## 1. The segment set

Segments are built by replaying blocks with a writable in-memory window in
front of an archive trie. Every node read or written goes into the window;
when the window fills it is sealed to disk. The segment accumulates exactly the
range's read-and-write set.

### 1.1 Decide the ranges

One `.todo` file per range in a chunks directory, named
`NNN-<firstBlock>-<lastBlock>.todo`. The name is the authoritative statement of
the range — the builder writes its output to a directory of the same name.

**Size ranges by captured bytes, not by block count.** Cost per block varies by
an order of magnitude across eras: the pre-fork era runs at ~168 blocks/s and
the Unitrie era at ~115, and within the pre-fork era the middle churns five
times more state than either end. Equal block counts produce segments from
0.2 GB to over 2 GB. Equal *bytes* produce segments that each fit in memory,
which is the property that matters — see §1.4.

If you are re-cutting an existing set, `segments.csv` inside each segment
records where its seals fell, which is a usable proxy for captured bytes.

### 1.2 Run the builders

```sh
cargo run --release -p rustock-cli --example build_segments -- \
  <worker-id> <block-dir> <archive-trie> <chunks-dir> <out-root> \
  <seal-mb> <tables> <progress-file> [--baseline <file>]
```

Workers claim chunks from `<chunks-dir>`, so launch several and they will not
collide. `seal-mb` × `tables` is the resident window per worker: size it
against free memory, not against core count. On a 7.7 GB box, three workers at
`700 2` is the practical limit, and a fourth has been measured as *worse* than
three — it contributed ~4% of throughput for 25% of the memory.

Every block's computed state root is verified as it is built: against the block
header at or above #1,591,000, and against `--baseline` below it.

**Without `--baseline`, a range below #1,591,000 is replayed but not verified**,
because the headers there carry Orchid roots. The builder warns; do not ignore
it. That is how the first pre-fork segment came to be built out of state nothing
had checked.

### 1.3 Chunks above the archive's reach

An archive is cumulative: its highest block says which *state roots* it can seed
from, not which nodes it holds. Replaying forward past that point works, because
replay creates the later nodes itself. Check where it ends with

```sh
cargo run --release -p rustock-cli --example archive_bound -- <block-dir> <archive>
```

But above that height a chunk can only be seeded from its predecessor's output,
which `build_segments` cannot reach — it takes one fallback store. So **the
backfill past the archive's end is inherently sequential**, one worker, and is
best shaped as a single long chunk resuming from its own checkpoint.

### 1.4 Verify, and only then trust

```sh
cargo run --release -p rustock-cli --example reexec_all -- \
  <block-dir> <segbuild-root> --threads N --baseline <file> --state <file>
```

This replays every segment from the segment alone. Two things to know:

- **A segment is only valid for the read set that built it.** Attach
  `--fallback <archive>` and it reports how many reads the segment could not
  serve, with the block and key for each, which is the repair list. Timings
  taken with a fallback attached are meaningless.
- **A segment cannot seed its own first block** unless that was handled: the
  parent state root is the one read in the whole build that bypasses capture.
  `probe_seed` detects it; `repair_chunk --limit 1` fixes it above the fork and
  `baseline_seed_repair` below it. Segments built after 2026-11-20 do not have
  the problem.

### 1.5 Publish a manifest

Selection must not be a directory glob. Retired and abandoned directories sit
beside live ones and some of their ranges *overlap* real segments; a reader that
opens one for a covered block misses every node, which under the
abort-on-missing-node rule looks exactly like a consensus divergence and is not
one. A live segment is named exactly `NNN-start-end`; anything with a suffix is
not one. Keep a manifest listing the live set and select from it.

---

## 2. The root baseline

One computed Unitrie root per block for #1..#1,591,000, 50,912,064 bytes,
fixed 32-byte stride so any block is a constant-time seek. That is what lets the
era be cut into segments at all, rather than replayed only from genesis.

### 2.1 From rskj's migration table (preferred)

```sh
cargo run --release -p rustock-cli --example baseline_from_rskj -- \
  <block-dir> <stateRoots-dir> <out-file>
```

Pure lookup: for each block, translate the header's Orchid root through the
table. Minutes of CPU, and **independent of anything rustock computes** rather
than audited against something independent afterwards. The anchor is taken from
#1,591,000's header directly, so the file's anchor is the chain's by
construction.

### 2.2 From rustock's own replay

```sh
# record the roots (replays from genesis) into <roots-db>
cargo run --release -p rustock-cli --example pre_unitrie_roots -- \
  <block-dir> <trie-dir> <roots-db> 1591000
# pack them, refusing a file whose anchor does not match the chain
cargo run --release -p rustock-cli --example baseline_build -- <csv> <block-dir> <out-file>
```

About four hours. `baseline_build` will not write a file whose last root
disagrees with #1,591,000's header, so no baseline can exist without having
passed its anchor.

### 2.3 Audit it

```sh
cargo run --release -p rustock-cli --example baseline_audit -- \
  <block-dir> <stateRoots-dir> <baseline>
```

Reports agreement block by block. A healthy result on mainnet is 1,590,999
checked, 0 differing, and exactly one block with no entry — **#1,591,000**,
which has no Orchid root to translate and is precisely the block the anchor
covers.

This is also how to detect a baseline that has gone stale. A deliberate
consensus change alters the roots it recorded; the schema version in the file
header catches a *format* change, but not that. Auditing does.

### 2.4 Repair a stale tail

```sh
cargo run --release -p rustock-cli --example baseline_rebuild -- \
  <block-dir> <trie-dir> <old-baseline> <from> <out-file> --scratch <dir>
```

Roots below the first affected block are still right, so only the tail needs
recomputing — `baseline_probe` finds that block in a handful of single-block
checks rather than a replay. The tool refuses to write unless the recomputed
tail arrives at the anchor.

---

## 3. Keeping the set current

Rebuilding from an archive every time the chain advances does not scale, so the
node extends the set itself — **not** by capturing as it syncs, but by deciding
where segments should be cut and re-executing each range afterwards.

The node syncs normally into its own store and adds each block's written bytes
to a counter. When enough has accumulated, the block number is recorded as a
boundary. Once the head is `--segbuild-lag` blocks past a boundary, a background
task builds that range by re-executing it from the node's own store.

```sh
rustock-cli --segbuild-out /path/to/segbuild \
            --segbuild-target-mb 1024 \
            --segbuild-lag 1000
```

Blocks are executed twice. At the tip that is close to free — a 1,000-block
range is about ten seconds of work per eight hours of chain — and it buys two
properties interception could not give:

- **The segment is exactly its range's read set.** A settled range of canonical
  blocks, re-executed in isolation, touches exactly the nodes it needs. Capture
  by interception also collected state from blocks executed beside the chain
  that never became canonical, so the segment stopped being the read set of its
  range.
- **It cannot produce a wrong segment.** The build verifies every state root and
  aborts on a missing node, so a failure leaves no segment rather than one that
  silently under-covers.

### 3.1 What the node checks before it starts extending

`--segbuild-out` must point at a set that is **current**, and the node says so
at startup rather than discovering it later:

- the directory must already hold at least one `NNN-start-end` segment — a
  continuous builder extends a set, it cannot bootstrap one, because the
  pre-fork era needs the baseline of §2; and
- the executed head must not be more than `--segbuild-lag` blocks past the set's
  end. A gap cannot be built from the node's store: that state has been
  collected. Fill it with §1 against an archive first.

### 3.2 How a pending range is kept alive

The node's trie store is garbage-collected, so a range waiting to be built has
to be protected, and it takes two things that each cover what the other cannot:

- a **collection bound** at the block the range seeds from, which keeps
  everything blocks after it wrote — including nodes created and destroyed
  inside the range, which no state reaches; and
- a **pin** on that same block's state root, which keeps the seed trie, whose
  nodes were written below the bound.

Both are derived from one number and registered automatically. A builder that
falls more than `--gc-max-bound-lag` blocks behind loses the protection — a node
out of disk is worse than an incomplete set. Once the collector has actually
swept beneath a pending range, that range is written off into `UNCOVERED`
*without being attempted*: the store's own watermark says its nodes are not
recoverable, so replaying it would only read into the hole. A build that fails
for any other reason — a full disk, a damaged store — is retried twice more and
then written off the same way. `UNCOVERED` is the set's list of known holes,
which §1 can fill from an archive.

See [`segbuild-continuous-design.md`](segbuild-continuous-design.md) for the
argument in full, and [`trie-capture-design.md`](trie-capture-design.md) for why
interception was abandoned.
