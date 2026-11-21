# Keeping the segment set current: deferred re-execution

**Status:** implemented. `rustock_storage::segbuild_boundaries` is the counter
and the logs, `rustock_execution::segment_build` is the builder, and
`--segbuild-out` turns it on. The retention mechanism is
`EpochTrieStore::collect_bounded`. What is not settled is §7.3.

The segment set covers `#1..#9,420,000` and the chain keeps moving. Rebuilding
from a 131 GB archive every time is not a plan, so the set has to extend itself
as the node syncs. This is how, and why it is not the obvious way.

Read [`trie-capture-design.md`](trie-capture-design.md) first if you want the
history: it is the retrospective on the previous attempt and the source of the
rules this design is trying to satisfy.

---

## 1. The obvious design, and why it was abandoned

Intercept the execution trie store, write every node to both the node's own
store and an open segment, copy every read into the segment, seal by size.
`crates/storage/src/segbuild_capture.rs` implemented exactly this, with a
high-water mark written in the same atomic batch as the nodes so a crash could
never leave the mark ahead of the data. Measured cost: **1.24x** per-block
execution, after batching per block brought it down from 7.6x.

It works, and it was merged. It is still the wrong design, and wiring it into
the node is what made that clear. Three problems, none of which is a bug in the
module:

**It captures uncles.** The node executes blocks beside the chain before
adopting them. Those executions read and write state through the same store, so
an intercepting segment accumulates nodes from branches that never became
canonical. The segment's range then claims to describe the canonical chain while
holding state from blocks that are not on it. Harmless for reads — nodes are
content-addressed — but the segment is no longer *the read set of its range*,
which is the only property that makes it verifiable.

**Fast-forwarded blocks break the mark.** `fast_forward_state` adopts a block
without re-running the EVM when it was already executed beside the chain. Its
nodes were written earlier, possibly in a previous process, possibly before
capture was enabled. Advancing the mark over such a block asserts coverage the
segment may not have. Three mitigations were considered and all of them push
complexity into the hot path. This is not rare: a single branch switch on
mainnet logged *"4 taken up without executing any again"*.

**Every operational event becomes a reconciliation case.** Restart, crash
mid-block, reorg past the mark, enabling the feature on a node with history —
each needs a rule, and each rule can only be tested by contriving the event.
A first implementation grew four reconciliation outcomes and a `GAPS` file
before the fast-forward question even surfaced.

The common thread: **interception makes the segment a side effect of whatever
the node happened to do**, including things that have nothing to do with the
canonical chain.

---

## 2. The design

Stop intercepting. Let the node sync normally into its own store, and build
each segment afterwards by **re-executing the range it covers**.

```
          node syncs normally, one store, no interception
                              │
          per block: add bytes-written to a counter
                              │
          counter >= target ──► record a boundary: "segment ends at block B"
                              │
          B + lag blocks later ──► background build of [A..B] from the
                                    node's own store, via build_segments
                              │
                      verified segment, sealed
```

Three moving parts:

**A byte counter.** Each executed block adds the bytes it wrote to the node's
trie store. Nothing is copied; the counter is the whole of the hot-path cost.

**A boundary log.** When the counter reaches the target the current block number
is appended to a boundary log and the counter resets. A boundary is a *decision*
about where a segment ends, recorded durably and cheaply, long before the
segment exists. Appending a number and fsyncing is the only durability the hot
path needs.

**A deferred builder.** Once the head is `lag` blocks past a recorded boundary,
a background task runs `build_segments` over that range, reading state from the
node's own store. Same tool that built the existing 161 GB set, which verifies
every block's state root as it goes.

### 2.1 What this buys

**The segment is exactly its range's read set.** A settled range of canonical
blocks, re-executed in isolation, touches exactly the nodes that range needs.
No uncles, no beside-the-chain execution, no dependence on what the node did
while it was syncing.

**It cannot produce a wrong segment.** `build_segments` verifies every state
root and aborts on a missing node. A failed build leaves no segment; it does not
leave a segment that silently under-covers. Compare the intercepting design,
whose failure mode was the hot path carrying on while the segment quietly lost
nodes — the §6.9d hazard, where a missing node is indistinguishable at the API
from a genuine absence.

**It is retryable and interruptible.** A build that fails or is killed can be
re-run from its boundary. Nothing is half-done, because the segment only becomes
real at the end.

**The hot path keeps one invariant instead of five.** No mark, no pending
buffer, no seal-at-boundary, no reconciliation, no fast-forward question. A
counter and an append.

### 2.2 What it costs

**Every block executes twice.** Once by the node, once by the builder.

At the tip this is close to free, and it is worth being precise about why: a
block arrives roughly every 30 seconds, and the builder re-executes recent
blocks at about 100 blocks/s with the segment resident. A 1,000-block range is
~10 seconds of work per ~8 hours of chain. The node is idle almost all of that
time; the second execution fits in the gaps.

During catch-up sync it is a real cost: the node is CPU-bound and the builder
competes with it. The builder may therefore fall behind and catch up later — but
not arbitrarily far. A pending range holds the collector's floor (§7), and past
`max_bound_lag` the floor is released and the state the range needs is
collected. The *decision* survives either way, because the boundary log is
durable; the state does not, and such a range has to be filled from an archive.

---

## 3. Is the state still there? The retention argument

The builder reads historical state from a **garbage-collected** store, `lag`
blocks after the fact. Whether that works is the question the whole design rests
on, and the answer is in `EpochConfig`:

```rust
Self { epochs: 4, burial_depth: 4_000, rotate_bytes: 1 << 30 }
```

A node survives until the epoch holding it is swept, and the collection root
sits `burial_depth` below the head. Consider the three kinds of node a
re-execution of `[A..B]`, `B = head - lag`, can need:

| node | still there? |
|---|---|
| written during `[A..B]` | only if its epoch has not been swept |
| written earlier, still in the state at some block of `[A..B]` | only if it is also in the state at the collection root |
| written earlier, already dead before `A` | not needed: it is in no state the range reads |

**The first two are not free, and `lag < burial_depth` does not give them.** An
earlier draft of this document claimed it did, by arguing that the collection
root sits below `B` and therefore below everything the range writes. The
collection root sits below `B`, but it sits *inside* `[A..B]`, because a segment
is far longer than the lag: with the defaults the root is at `head - 4,000`
while `A` is around `head - 90,000`. So

- a node written at `A + 5` and dead by `A + 9` is reachable from no state at
  all, including the collection root's, and
- a node present in the state at `A - 1` but superseded at `A + 100` is not in
  the state at the collection root either.

Both are read by the re-execution. Nothing in the collector keeps them unless
it is told to.

So retention is **arranged, not argued**: the builder registers a bound and a
pin, and §7 gives the mechanism and why one without the other is not enough.
The short version is that the two halves are exhaustive — a node read while
re-executing `[A..B]` was either already in the trie at `A - 1`, which the pin
keeps, or written by a block at or after `A`, which the bound keeps — and that
`SegmentBoundaries::collection_bound` is the single place that computes both.

Granularity is epoch-level, which is coarser than necessary and therefore
conservative: it keeps more than asked, never less.

The sweep gate this builds on is not a soft guarantee. It is enforced in
`collect_inner`, with a recorded incident behind it: seeding a store with one
epoch, executing 3,800 blocks into it and collecting against a root below those
blocks destroyed exactly that state, and execution went on to read zeros and
compute 176,127 gas for a block whose header said 328,664.

### 3.1 What still has to be handled

The guarantee is about the *collector*, not about everything that can go wrong.
A build can still fail to read a node — a corrupted store, a manual collection
forced against a bad root, a `lag` configured above `burial_depth`. The builder
must therefore treat a missing node as what it is:

- **abort the build for that range** (it already does), and
- **record the range as uncovered**, so the hole is a fact on disk rather than a
  gap someone notices later by comparing segment names.

The set then has a known hole that can be filled from an archive, rather than a
segment that lies.

---

## 4. Components and contracts

### 4.1 The counter

Signal: **bytes written to the trie store per executed block**, not a block
count. Equal block counts produce segments from 0.2 GB to over 2 GB because cost
per block varies tenfold across eras; equal bytes produce the ~1 GB segments
that fit in a consumer's memory, which is the property that makes replaying one
fast.

The counter is approximate by design. It drives *where to cut*, and a cut in
roughly the right place is all that is needed.

### 4.2 The boundary log

Append-only, one line per boundary, fsynced:

```
<block>\t<approx-bytes>\t<unix-time>
```

Durability requirement: a boundary must survive a crash, because losing one
means two segments merge into an oversized one. Durability is cheap here —
one line per gigabyte of state written.

A boundary is **advisory until a build succeeds**. If a build fails, the
boundary stays in the log and the range stays unbuilt. After three failures it
is written off into a third log, `UNCOVERED` — §3.1's requirement that a hole be
a fact on disk, and what lets the builder carry on with later ranges instead of
looping on the one it cannot do.

**Where the cut decision is made: by the background task, at the executed
head.** The counter is incremented by the store wrapper, per write. The decision
to *cut* is taken by the same task that builds, which reads the node's executed
head and calls `end_block` on it.

This needs no hook in the execution loop, and it cannot name a block that has
not been executed — which matters, because a segment can only be built from
state that exists. The cost is that a cut lands on the first poll after the
target is crossed rather than exactly on it, which is acceptable for the same
reason the counter is approximate: sizes are advisory.

Only the handle the block processor writes through is wrapped. The readers'
handle is not: an `eth_call` writes speculative state through the same store,
and counting it would cut segments early for no reason.

### 4.3 The deferred builder

`rustock_execution::segment_build`, which is the library form of the
`build_segments` example that built the existing 161 GB set, reduced to the one
case a running node needs: one range, no chunk claiming, no worker pool, no
baseline. It runs when `exec_head >= boundary + lag`. For range `[A..B]`:

1. seed from the state root at `A - 1`, read from the node's store **and put
   into the window as it is read** — this read is the one in the whole build
   that bypasses copy-on-read, and leaving it out is why 96 of the original 97
   chunks could not replay their own first block;
2. replay `A..B`, verifying every state root against its header;
3. seal the window;
4. rename `.building-A-B` to `NNN-A-B`;
5. record the range built, with the canonical hash at `B`.

Constraints:

- **One build at a time.** Two concurrent builds double the memory and compete
  with the node for the same store.
- **Read-only on the node's store.** Everything read is copied into the window,
  and the window is what is written.
- **Bounded memory.** Two in-memory tables of `--segbuild-seal-mb`, which is the
  builder's budget and not the segment size: a segment is the whole range, and
  sealing a table merely flushes it into the segment's store. Three workers at
  700 MB x 2 tables was the measured ceiling on a 7.7 GB box, and a fourth was
  *worse* than three.
- **Killable.** A killed build leaves `.building-A-B`, which no reader parses as
  a segment, and resumes from its last checkpoint.

**Step 4 is the commit.** Nothing that reads the set can see a half-built
segment, because until the rename there is no name that parses as one. This is
what makes "either a correct segment or no segment" structural rather than a
property of the error handling: the mismatch path, the missing-node path, the
cancel path and a SIGKILL all leave the same thing behind.

### 4.4 Numbering, and not colliding with the existing set

The existing set uses `000`–`097` and `900`–`908`. A continuous builder must
take indices that cannot collide, and must continue across restarts rather than
restarting at `000` — an index that repeats stops indicating order, which is
what made two abandoned directories sharing an index hard to diagnose (§11 of
`trie-segments-design.md`).

Take the highest existing index and continue. Only a name that is exactly
`NNN-start-end` counts; a suffixed name is retired and must not influence
numbering.

---

## 5. Failure modes

| event | what happens |
|---|---|
| node restarts | counter resets, so one segment is cut late. Harmless: sizes are advisory |
| crash mid-build | `.building-A-B` stays behind, which no reader parses as a segment; the build resumes from its last checkpoint |
| reorg below a boundary | the boundary is above `head - lag` and so not yet built; re-record it |
| reorg below an already-built segment | only possible for a reorg deeper than the lag. A `HALTED` file is written and building stops until a human clears it (§7) |
| missing node during a build | abort; after three attempts the range is written off into `UNCOVERED` and stops holding the floor |
| builder falls behind | ranges queue up and hold the collector's floor. Past `max_bound_lag` the floor is released and those ranges fail on missing state, each written off after three attempts |
| disk fills | build fails, boundary stays unbuilt, node unaffected |

The shape worth noticing: every failure leaves either a correct segment or no
segment. None leaves a segment that claims more than it holds.

---

## 6. What this replaces

`crates/storage/src/segbuild_capture.rs` is deleted. Its crash-safety
machinery — the high-water mark in the same batch as the nodes, the per-block
pending buffer, seal-at-a-block-boundary — solves problems this design does not
have.

**Deleted rather than left in place.** 471 lines of unused dual-write code
implies it is the supported path. Its measurement is worth keeping, and is
recorded here: interception costs 1.24x per-block execution after per-block
batching, and 7.6x without.

---

## 7. Decisions and open questions

**A reorg below a built segment: decided — halt and require a human.**

Nothing is built until it is `lag` blocks settled, so a reorg can only reach a
built range by being deeper than the lag. On mainnet that is more than a
thousand blocks abandoned, which is not an operational event: it means a
majority of miners rewrote settled history, and the social contract has failed
before the segment set has. Recovering gracefully from it would be designing
for a world in which the chain's assumptions do not hold.

**The builder stops. The node does not.** This is the whole meaning of
fail-stop here, and it is worth stating because the phrase invites the opposite
reading: the node must keep following the chain, keep serving RPC and keep
answering queries, because that is exactly how an operator finds out what
happened. A node that stopped would remove the instrument needed to diagnose
the event.

That has a hard consequence for the implementation. **Nothing in the segbuild
path may be fatal to the node.** Not a halt, not a failed build, not a full
disk, not a panic in a builder thread. Concretely: every lock in
`segbuild_boundaries` is taken poison-tolerantly, so a panic in a builder
thread cannot poison a lock and then panic the node's execution path on the
next block; and the builder's errors must be logged and dropped by the caller
rather than propagated into sync. Extending the segment set is a by-product.
Failing to extend it is not a reason to stop following the chain.

`BUILT` records the canonical hash at each range's end
block, every tick compares it against the chain's current hash at that height,
and a mismatch — or a height that no longer has a canonical block at all —
writes a `HALTED` file with the reason and stops building. Nothing is built
while that file exists, it is not self-clearing, and it survives a restart: the
fault is in the chain rather than in this process, so a builder that resumed on
its own would quietly produce segments across a discontinuity.

Clearing it is deliberate and manual. Investigate, repair the set if needed,
remove the file.

The check is cheap — one canonical-hash lookup per built range — and is only
meaningful because of the lag. Without it, ordinary shallow reorgs would trip
the alarm constantly.

**Protecting a pending range: decided — a bound *and* a pin.**

An earlier draft of §3 argued the state is retained anyway. It is not, so this
is not a nicety: without it the builder reads a hole.

**A pin alone is the wrong shape.** `collect_pinned` keeps everything
*reachable from a state*, which is what the snapshot checkpoint needs. A segment
needs every node its *block range touched*, including nodes created and
destroyed inside the range, which no state at either end reaches. Pinning the
seed root at `A - 1` protects the seed and misses those.

**A bound alone is also not enough**, and this is the part that is easy to get
backwards. A bound keeps what blocks *after* it wrote. The seed trie at `A - 1`
was written *before* it, and a node of that trie that is superseded at `A + 100`
is in no state the collector marks — not the collection root's, since the root
sits at `head - 4,000`, well above `A`. The bound does not keep it.

**So both, and they are exhaustive.** A node read while re-executing `[A..B]`
was written either before `A` or at/after `A`. If before, and it is still needed,
it is in the trie at `A - 1` — that is what "still needed" means — and the pin
keeps it. If at or after, the bound keeps it. One block number,
`A - 1`, produces both: the bound is that number, the pin is the state root the
header at that number declares. `SegmentBoundaries::collection_bound` returns
it, and the GC cycle turns it into one bound and one pin.

**Why a bound is the right shape for its half**: "the nodes a block range
touched" is a statement about blocks, and the epoch store already tags writes by
block. Each epoch records `last_block`, and the sweep gate is already a block
comparison:

```rust
match oldest.last_block {
    Some(b) if b <= root_block => {}   // safe to sweep
    Some(b) => bail!("refusing to sweep: ... writes up to block {b} ...")
```

So the mechanism is to generalise that single threshold into a minimum:

```
threshold = min(root_block, every registered bound)
sweep the oldest epoch only if  epoch.last_block <= threshold
```

The collector's own `root_block` is simply the bound it imposes on itself.

Bounds are supplied the way pins are, by a callback evaluated per cycle
(`gc_bounds` alongside `gc_pins`), so nothing is stored and nothing has to be
revoked: a range stops holding the floor the moment it is built and drops out
of the pending set. Persistence across restarts is free, because the pending
ranges are the difference between `BOUNDARIES` and `BUILT`.

In steady state the bound costs nothing. The builder is `lag` blocks behind, so
the lowest unbuilt range starts about one segment back, while the oldest of four
one-gigabyte epochs holds writes from about three segments back — comfortably
below the floor, so the sweep proceeds as it always did. The bound bites only
when the builder falls behind by epochs rather than blocks, which is exactly
when it should.

### 7.1 A bound that stops advancing must not fill the disk

A bound blocks collection. A builder that is stuck, crashed, or simply far
behind therefore holds the floor down, and the store grows without limit. By
the rule in §7 that is not allowed to harm the node, so a bound has to be
accountable:

- **Report it.** Every cycle, log which bound holds the floor and how far below
  the head it sits. A floor drifting away from the head is the symptom. A held
  floor is reported as an *outcome*, not an error: `CollectStats::held_by_bound`
  names it and the cycle returns `Ok`, because a cycle that failed once a minute
  would bury the failures that matter.
- **Override a stale one.** Past a configured distance the bound is ignored,
  loudly. The reasoning is `CollectStats::unservable`'s: an incomplete segment
  set is a thing to repair later, a node out of disk is not.

The override must be loud enough to be acted on, because the consequence is a
gap in the set. It is the same trade as refusing to abort a cycle for an
unservable pin: do not turn one broken thing into two.

**Interaction with the snapshot checkpoint pin.** Both want state kept alive,
and they compose by construction: the live set is a union, bounds are a minimum,
and the segment's own seed root joins the snapshot roots in the same pin list.
It has not been *measured* with both active, which is the part still open — the
cost of a pin is the delta between it and the collection root, and a seed root
one segment back is a larger delta than a snapshot checkpoint 10,000 blocks
back.

### 7.2 Enabling it needs a current set, and that is checked at startup

A gap between where the set ends and where the node has executed cannot be
built from the node's store: the state those ranges read is long collected. So
`--segbuild-out` refuses at startup if

- the directory holds no `NNN-start-end` segment at all — a continuous builder
  extends a set, it cannot bootstrap one, because the pre-fork era needs a
  baseline the node does not have; or
- the executed head is more than `lag` blocks past the set's end — the operator
  fills the gap with the offline tool against an archive first.

Refusing at startup rather than at runtime is the one place where segbuild may
stop the node, and it is not really an exception: nothing is serving yet, the
flag was explicitly asked for, and the alternative is a node that looks
configured while recording ranges that must fail.

### 7.3 Still open: the builder during a long catch-up

The startup check in §7.2 keeps a *gap* out of the picture, but it cannot stop a
node that starts current from falling behind afterwards — a week of downtime, a
partition, a stall. Then the node executes forward while the builder re-executes
the same blocks a second time on the same box, so the builder loses ground at
roughly one block for every block gained.

The arithmetic says how much that costs, and it is less than it first looks:

| quantity | default | as chain time |
|---|---|---|
| `--gc-max-bound-lag` | 12,000 blocks | ~4 days |
| one segment | ~90,000 blocks | ~31 days |

A pending range keeps its protection until the builder is `max_bound_lag` behind
the executed head. Past that the floor is released, the range's seed is
collected, and the build fails — on its *first* lookup, before any replay, so
three attempts cost three lookups rather than minutes. The range is then written
off into `UNCOVERED` and the builder moves on.

So the consequence of leaving this alone: a node resuming from more than roughly
a week of downtime records a hole for each segment it passed, cheaply and
loudly, and the holes are filled from an archive with §1. Not a cascade, not
expensive, but not free either.

**Decided — skip, don't attempt.** The builder asks the store what it has
already collected, and a range the collector has swept beneath is written off
without being replayed.

The predicate is the store's own **collection watermark**
(`EpochTrieStore::collected_below`), not the builder's distance from the head.
That distinction is the whole point: distance says a range *lost its
protection*, which is not the same as having been collected — nothing is gone
until a sweep actually reaches it, and a range that still has its nodes should
still be built. The watermark is a recorded fact with exactly the meaning
needed: a node missing beneath it was taken by the collector and is not
recoverable from this store, while a node missing above it is damage. So the
test is `collected_below >= range.start`, and a range that passes it is
recorded in `UNCOVERED` with that reason, releases the floor, and is never
offered again.

This costs nothing in the normal case — the watermark trails the collection
root, which trails the head — and in the bad case it turns minutes of replay
into a lookup, three times over into once, and a confusing mid-build failure
into a sentence an operator can act on.

**Still open — idle-only.** Should the builder stop building entirely while the
node is more than `lag` blocks behind the best header? It would free CPU for
catch-up exactly when the node needs it, at the cost of guaranteeing the holes
that the rule above merely records sooner. That is a policy decision about a
production node rather than a code question: it trades segment coverage for
sync speed, and which an operator wants depends on why their node is behind.
