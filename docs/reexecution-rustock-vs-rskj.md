# Re-executing mainnet from segbuild: rustock against rskj

**What this is.** A description of the segbuild segment format, how to read it
from rskj, and a measured comparison of how fast rustock and rskj re-execute
mainnet blocks out of it.

**Written for rskj developers.** §1–§3 assume no familiarity with rustock or
segbuild and are the parts to read if you only want to use the database. §4–§9
are the measurements and how they were taken.

**Measured 2026-11-20** on a 4-core, 7.7 GB box with the mainnet node stopped
for the duration. Both clients read the same segment store, both holding the
segment in memory. Rates here are specific to that hardware; the ratios between
the two clients are the transferable part.

---

## 1. What a segbuild database is

Re-executing RSK mainnet needs the historical state each block read. The
straightforward way to have it is an archive trie — every node ever written,
which for mainnet is about 131 GB and growing. That is awkward to move, awkward
to share, and most of it is irrelevant to any particular block.

A **segbuild database** is the same history cut into **range-scoped segments**.
One segment covers a contiguous range of blocks and holds the trie nodes those
blocks *touched* — nothing else. Segments tile the chain with no gaps and no
overlaps, so exactly one segment covers any block, and re-executing a range
needs only its own segment rather than the whole archive.

The current mainnet set:

```
106 segments, blocks #1..#9,420,000, 161 GB total
median segment 1,265 MB, largest 2,718 MB
```

Two properties make this useful rather than merely smaller:

- **A segment is small enough to hold in memory.** At ~1.3 GB a segment fits in
  heap, and a trie walk is a chain of *dependent* reads — the next node's hash
  is inside the node just read, so nothing can be prefetched and every read's
  latency is paid in full. Holding the segment in memory removes that latency
  entirely, which is worth far more than the size reduction. Both clients in
  this report do it.
- **Segments are independent.** Different ranges can be re-executed on
  different machines, or in parallel on one, with no shared state.

### 1.1 The property that will bite you

**A segment holds what its producer read.** Not the state at those heights —
what the client that built it actually touched while executing those blocks.

A read set is a property of an implementation, not of the chain. If your client
reads a node the producer did not, the segment cannot serve it, and a trie walk
that dies on a missing node is **indistinguishable at the API from one that
reached a genuine absence**: both return null. So an incomplete segment does not
fail loudly, it answers wrongly, and the state root comes out different.

That is why segment readers must treat a missing node as an error and abort,
never as "absent". It is also why §6 exists: the first attempt to read these
segments from rskj failed, and the cause was rskj asking for state no consensus
rule required.

---

## 2. How the set is generated

Blocks are replayed forward with a writable in-memory window in front of an
archive trie. Every node read or written during the range goes into the window;
when the window reaches a size threshold it is sealed to disk. The effect is
copy-on-read: the segment accumulates exactly the range's read-and-write set.

```
build_segments <worker-id> <block-dir> <archive-trie> <chunks-dir>
               <out-root> <seal-mb> <tables> <progress-file>
               [--baseline <file>]
```

Work is claimed per chunk from `<chunks-dir>`, so several workers build
different ranges concurrently and an interrupted build resumes from its last
checkpoint. Every block's computed state root is verified as it goes: against
the block header above the Unitrie fork, and against a recorded-roots baseline
below it (pre-RSKIP126 headers carry Orchid-format roots, which are not Unitrie
roots and cannot be compared to one).

### 2.1 On-disk layout

```
<segbuild-root>/
  MANIFEST                        authoritative segment list — select from this
  NNN-<firstBlock>-<lastBlock>/
    sealed/                       RocksDB, column family "trie_nodes": hash -> node
    segments.csv                  seq,start_block,bytes — where the seals fell
    checkpoint                    resume point, present only if the build was interrupted
  <name>.superseded-*             retired, NOT part of the set
```

**Select segments from `MANIFEST`, never from a directory glob.** Retired and
abandoned directories sit beside live ones and some of their ranges *overlap*
real segments. A reader that opens one of those for a covered block misses
every node in it, which — under the abort-on-missing-node rule above — looks
exactly like a consensus divergence and is not one. The name of a live segment
is exactly `NNN-start-end`; anything with a suffix is not one.

---

## 3. Reading a segbuild database from rskj

### 3.1 The branch

Not upstream. The implementation lives on

```
https://github.com/SDL-A1/rskj   branch feature/segbuild-resident-arena
```

which adds a read-only `TrieStore` over a segment set, an in-memory
representation of a whole segment, and the fix in §3.3 without which none of it
works.

### 3.2 Running it

```sh
java -cp <rskj> co.rsk.cli.tools.ExecuteBlocks   --fromBlock <n> --toBlock <m>   --trieStore SEGBUILD   --segbuildDir  /path/to/segbuild   --segbuildResident   -Xdatabase.dir=/path/to/rskj/database --main
```

| flag | |
|---|---|
| `--trieStore SEGBUILD` | read state from a segment set instead of the node's own unitrie |
| `--segbuildDir` | the segment set's root |
| `--segbuildResident` | load each segment wholly into heap and close its database. Costs about the segment's size in heap — size `-Xmx` accordingly, 4 GB for the largest in the current set |
| `--segbuildFallback` | serve a miss from the node's own unitrie and count it, instead of failing. For diagnosis only: it makes timings meaningless, and `getFallbackHitCount()` then tells you how far the segment falls short of your read set |

The range should sit inside one segment; crossing a boundary makes the run load
a second one partway through.

**What the node's own database still has to provide.** Segments supply state,
not blocks, so a replay needs `blocks/` (including its MapDB `index` file) and
`stateRoots/` from an rskj database. It does *not* need `unitrie/` — that is
what the segments replace. `receipts/` must exist and be openable because
`TransactionExecutorFactory` takes the receipt store, but re-execution writes
receipts rather than reading them, so an empty RocksDB there is enough.
Read-only access is RocksDB-only, so a LevelDB database cannot be replayed
read-only.

### 3.3 The prerequisite: demand-populate the BTC block cache

**A stock rskj cannot read a segment set at all.** The patch is

```
docs/rskj-patch-demand-cache.md        (in the rustock repository)
```

applied as the first commit on the branch above. In short:
`RepositoryBtcBlockStoreWithCache.populateCache` walked back up to 5,000 blocks
from every new chain head calling `get()` on each, reading repository state no
consensus rule required. Against a segment set that is fatal, because a segment
holds only what consensus demanded.

It is close to performance-neutral for a normal node — the walk stopped at the
first cached block, so it paid in full about once, at startup. What it buys is a
**deterministic read set**: the nodes a block reads become exactly the nodes
consensus required. That is interesting beyond segbuild, for state proofs and
for snapshot-style sync.

The patch also establishes a rule worth keeping in its own right: **never cache
an absence.** A repository read that cannot find a node and a genuinely absent
value are the same `null`, and nothing at that layer can distinguish them, so
recording an absence lets partial state produce a confidently wrong answer for a
later consensus-required lookup.

---

## 4. The headline, and which number to quote

| | replay only | chunk load (2,468 MB) | wall |
|---|---|---|---|
| rustock | **100.60 blk/s** (99.40–101.01) | 8.1 s | 58.4 s |
| rskj | **62.19 blk/s** (61.65–66.31) | 91.2 s | 172.9 s |
| ratio | **1.62x** | 11.4x | 2.96x |

Blocks #9,410,000–#9,414,999 (5,000 blocks) from segment
`097-9230009-9420000`, three repeats each, interleaved, medians.

**Quote 1.62x.** It is the only one of the three that compares the thing the
question is about — how fast each client executes blocks.

The 2.96x wall figure is mostly rskj's loader, and at the time of measurement
that loader was unoptimised Java of our own writing (§8). Quoting it would be
reporting our inefficiency as an rskj property.

---

## 5. Why the range is 5,000 blocks and not 50

A 50-block pilot of the same work reported **7.1 blk/s** for rskj, which would
have made rustock look **13x** faster. That number is an artefact: the JVM
interprets bytecode before the JIT compiles it, so a short run measures the
interpreter. Java needs thousands of iterations before it is running the code
it will actually run.

Sizing the range for convenience would therefore have produced an eightfold
exaggeration in rustock's favour, with nothing in the output to suggest
anything was wrong. This is the single most important methodological point in
this document: **any comparison against a JVM on a short range is invalid, and
invalid in the direction that flatters the native implementation.**

Two further rules the measurement follows:

- **Interleaved, not batched.** rustock, rskj, rustock, rskj... Page cache and
  thermal state drift over tens of minutes; running all of one client's repeats
  together hands that drift to whichever went second.
- **Load reported apart from replay.** Loading the chunk costs 8–91 s depending
  on the client. On a one-minute run that is most of the wall clock, so adding
  the two together measures loaders and calls the result execution speed.

The runner is a local script (`ops/benchmark-reexec.sh` on the measuring box,
not in either repository). It refuses to start if a node is running, or if the
block range crosses a segment boundary — which would make one run pay for a
second load partway through.

---

## 6. How the read-set problem was found

§3.3 gives the fix; this is how it surfaced, because the shape of the failure
is worth knowing if you meet it again.

The first attempt failed at block #9,229,646 with a missing node, which looked
like an incomplete chunk and was not. `RepositoryBtcBlockStoreWithCache`
called `populateCache` on every `setChainHead`, walking back up to 5,000 blocks
and calling `get()` on each — **reading repository state no consensus rule
required**.

A segbuild chunk holds what its *producer* read. Any client that reads more
than consensus demands cannot be served by one, however complete the chunk is.
The defect was not that rskj read too much; it is that reading anything
unasked-for makes a segment store unusable.

The fix is `docs/rskj-patch-demand-cache.md`, applied as
`feature/segbuild-resident-arena` on `SDL-A1/rskj`:

- **populate only from reads that happened anyway** — nothing walks ahead;
- **never record an absence** — a repository read that cannot find a node and a
  genuine absence are the same `null`, so caching an absence lets partial state
  produce a confidently wrong answer.

With it, the same range replays clean. **That portability is a more important
result than any timing in this document:** it is what makes the segment
databases usable by a second implementation.

### 6.1 One correction to that patch

The document's §4.1 had `get()` answer from the cache. That is wrong, and
rskj's own `RepositoryBtcBlockStoreWithCacheTest.cacheLivesAcrossInstances`
fails on it: one `Factory` owns one cache and hands it to every store it makes,
while each store holds its own `Repository` track, so answering from the cache
lets one track report a block present that only another track wrote. It is the
mirror of the absence rule — reporting a presence the caller's own state does
not have.

`get()` therefore populates but does not answer. Nothing is lost, because
`getStoredBlockAtMainChainDepth` already consults `getFromCache` before falling
back. §4.1, §4.6 and §5.1 of that document are corrected and dated.

---

## 7. The whole chain, at 8 GB

### 7.1 What is measured rather than projected

rustock's whole-chain figure is not an estimate. The runs happened:

| range | blocks | threads | wall | aggregate |
|---|---|---|---|---|
| pre-fork, #1..#1,591,000 | 1,590,992 | 2 | 1.5 h | 303.5 blk/s |
| Unitrie era | 7,638,848 | 2 | 11.3 h | 187.3 blk/s |
| **whole chain** | **9,229,840** | **2** | **12.8 h** | — |

Serial, from 105 individual segment measurements: **21.4 h at 122.4 blk/s**.
By era:

| era | blocks | serial rate | serial time |
|---|---|---|---|
| pre-fork (Orchid) | 1,590,992 | 167.8 blk/s | 2.6 h |
| early Unitrie | 2,443,044 | 117.6 blk/s | 5.8 h |
| mid | 2,994,432 | 114.2 blk/s | 7.3 h |
| recent | 2,391,363 | 116.8 blk/s | 5.7 h |

Pre-fork blocks run ~45% faster: the state is small and the whole working set
stays close.

### 7.2 Threads are bounded by memory, not cores

A worker holds one segment resident. The segment set has a median of 1,265 MB
on disk and a maximum of 2,718 MB (`097`), so with ~0.95x for data plus
per-client overhead:

| | per worker, median segment | per worker, largest segment |
|---|---|---|
| rustock | ~1.9 GB | ~3.3 GB |
| rskj | ~2.0 GB | ~3.7 GB |

At 8 GB, leaving 1 GB for the OS and page cache, that is **3 workers on a
typical segment**, falling to 2 (rustock) or 1 (rskj) while the largest segment
is resident. Adding cores does not help past that, and the original build
already measured four workers as worse than three: the fourth contributed ~4%
of throughput for 25% of the memory.

Measured two-thread speedup is **1.81x** (pre-fork) and **1.62x** (Unitrie) —
not 2x. Memory bandwidth and page cache are shared.

### 7.3 The estimate

Taking 3 threads at a ~2.2x speedup (extrapolating the measured 1.6–1.8x at
two, and bounded by the four-is-worse-than-three finding):

| | serial | 2 threads | 3 threads |
|---|---|---|---|
| rustock | 21.4 h *(measured)* | **12.8 h** *(measured)* | ~10 h *(est.)* |
| rskj, replay only | ~34.6 h | ~20 h | ~16 h |
| rskj, + load at 91 s/segment | ~37.3 h | ~21 h | ~17 h |
| rskj, + load after §8 | ~36.0 h | ~20 h | ~16 h |

rskj's replay figure scales rustock's measured per-era times by the observed
0.618 rate ratio. **That is the weakest assumption here**: the ratio was
measured on recent blocks only, and the two clients' relative cost could differ
by era — Bridge-heavy ranges in particular. It is a projection, not a
measurement, and it is the one number in this document worth re-measuring
before relying on.

**Practical answer: rustock re-executes the chain in about half a day on this
hardware; rskj in about one day.** Both are memory-bound at three workers, and
neither benefits from more cores.

---

## 8. Why rskj's loader was 11x slower, and what fixed it

92 s for 2,468 MB is 27 MB/s — nowhere near the disk. The time was garbage
collection, and the cause was ours.

`RocksIterator.key()` and `.value()` each allocate a fresh `byte[]`. The
loader made two passes — one to size the arenas, one to fill them — and the
sizing pass allocated two arrays per node *purely to read their lengths*. For
a 25.3M-node chunk:

```
  pass 1   50,672,490 allocations, discarded immediately
  pass 2   50,672,490 allocations
  total   101,344,980 throwaway arrays, ~2.3 GB of headers
```

rustock's loader, by contrast, is single-pass (sized from
`estimate-num-keys` with 5% slack), parallel across leading-byte ranges, and
appends into pre-reserved buffers.

The fix uses APIs that were already there:

- **`key(byte[], int, int)` / `value(byte[], int, int)`** read into a
  caller-supplied array at an offset, so **the arena itself is the destination**.
  Zero allocation per node, one unavoidable copy.
- **`estimate-num-keys`** sizes the index, with a rehash if the estimate proves
  low — one rehash at worst against a guaranteed second scan.
- **`total-sst-files-size`** sizes the arenas, which shrink with the remainder.

Measured on segment 097, with the node running (so under *worse* conditions
than the 91.2 s baseline, which was taken with it stopped):

| | load | heap for 2,468 MB |
|---|---|---|
| two-pass, allocating | 91.2 s | 2,980 MB (1.21x) |
| single-pass, zero-alloc | **44.6–56.4 s** | 2,980 MB (1.21x) |

**Roughly 1.8x faster at no cost in heap.** The spread is page-cache contention
from the running node; the baseline had the box to itself, so the comparison is
conservative.

Still short of rustock's 8.1 s, and the remaining gap is **parallelism**:
rustock shards the load across leading-byte ranges and this does not. That is
the obvious next step and is not done.

Arenas are sized from `total-sst-files-size`, which counts index and filter
blocks as well as keys and values — it reports 2,718 MB for 2,468 MB of data —
so the final arena is trimmed to what was used. Without that trim the heap is
3,228 MB (1.31x); with it, the single-pass loader costs exactly what the
two-pass one did.

### 8.1 Three bugs, all caught by assertions rather than review

Worth recording, because the arena rewrite was the most error-prone work in
this exercise and none of the three was visible by reading the code:

- A full 1 GiB arena allocated up front, giving **1,024 MB of heap for 1 MB of
  data**. Caught by the test asserting heap < 2x data.
- Full-size arenas for all but the last, so 1,050 MB of data took **2,048 MB**.
  The segments sit exactly on that boundary, so every one of them doubled; the
  test JVM died against `-Xmx4G`.
- `Integer.MAX_VALUE` used as a "does not fit" sentinel, which the very next
  size check read as a 2 GB node and rejected.

The correctness test compares **every node** in a real chunk against the
database it came from, not a sample: an open-addressing bug surfaces on
colliding keys, which are by definition the ones a sample is least likely to
contain.

---

## 9. What this does not answer

- **rskj's per-era rate.** Measured on recent blocks only; §7.3's whole-chain
  figure for rskj inherits that assumption.
- **Beyond three workers.** Untested above 3, and bounded by memory rather than
  by scheduling, so a larger box would change the answer.
- **The loader's remaining 5x.** Sharding the load across threads is
  unimplemented.
- **Anything about a node under live load.** Both clients were replaying
  historical blocks from a sealed store with the node stopped. This says
  nothing about sync throughput.
- **Whether 1.62x holds elsewhere in the chain.** One range, 5,000 blocks, in
  the most recent segment.
