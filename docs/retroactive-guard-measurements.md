# Measuring whether a guard can be removed

A guard over a window that can never reopen is a candidate for deletion. The
condition is not that the window is closed — it is that **nothing in the window
ever took the branch**, which is a claim about this chain's history and not
about the code. Removing a guard on reasoning alone changes the state root this
node computes for blocks that are already on the chain.

This document records what has been measured, with the exact command for each
and how to widen it. It is a draft: most rows are instrumented and unrun, and
the two results that exist are marked as such.

## The rule that decides what needs measuring

A guard may be removed **without** measurement only if the branch it guards
makes the whole *block* invalid, because then no canonical block can contain
one. Every other kind needs the measurement — an opcode "not available at this
height" halts the frame and consumes its gas, and the block is valid and in the
chain; a bridge method a caller is not permitted to call throws and charges the
whole gas limit, and the block is valid and in the chain. Per `BR-031` a direct
bridge failure is not even marked in the receipt, so the only historical trace
is a gas figure.

Three classes follow. **A** — the branch invalidates the block; removable with
no measurement. **B** — the branch yields a valid block with a different
result; removable only if measured. **C** — the window is empty on a given
network, so there is nothing to measure and the code stays for the networks
where it is not.

## What has actually been run

### `EVM-073` — is `v = 34` anywhere in the chain?

`v` is one octet, decomposed as `chainId = (v - 35) / 2` with the subtraction
floored at zero, accepted set `27, 28, 34 … 255`. `v = 34` is accepted and means
exactly what `28` means, and is not a value EIP-155 can produce. Narrowing the
set to `27, 28, 35 …` would be a de-divergence. Class **B**: a single `v = 34`
kills the proposal, because such a transaction sits in a valid block whose
sender was recovered from it.

**Result: settled for mainnet. No `v = 34` anywhere**, across every body this
node has stored — 12,799,779 of them, 31,210,944 transactions, none
undecodable, 219 seconds.

```
            v        count  meaning
            0   12,799,778  no signature
           27      951,612  legacy, pre-EIP-155
           28      951,791  legacy, pre-EIP-155
           95    8,251,088  chain 30, mainnet
           96    8,256,675  chain 30, mainnet
```

Exactly five distinct values. Three things in that table are worth keeping:

- The `v = 0` count is one per body but for a single exception, which is REMASC
  appearing in every block. It is the check that the instrument read real
  transactions rather than finding nothing because it was broken.
- **1.9 million transactions are pre-EIP-155**, signed with 27 or 28 — not a
  handful of early curiosities. An exact walk of #1..#150,000 found only nine of
  them, so they belong to later eras, and anything that assumes a chain id can
  be recovered from `v` is wrong for 6% of this chain.
- No value in `29..=33` appears. Those are refused by `EVM-073`, so one would
  have been a finding about the store or the decoder before it was one about the
  chain.

12,799,779 bodies against 9.43 million canonical blocks means roughly 3.37
million stored bodies are not on the chain — which is why the screen is a
superset, and why a clean screen is worth more than a clean canonical walk: it
rules out the value in blocks the chain abandoned as well.

```sh
# exact, canonical blocks only -- about 370 blocks/s
cargo run --release -p rustock-cli --example tx_signature_v -- \
    /var/lib/rustock --from 1 --to 150000

# screen: every stored body, canonical or not -- about 57,000 bodies/s
cargo run --release -p rustock-cli --example tx_signature_v -- \
    /var/lib/rustock --screen
```

**Widen the range** with `--from` and `--to`; omitted, they are genesis and the
node's head. The screen always covers everything the node stored, so for this
rule there is nothing left to widen on mainnet — only testnet remains.

**The screen is 160x faster and one-directional.** Measured: 58,800 bodies/s
against 370 blocks/s for the exact walk, so the whole chain takes under four
minutes rather than seven hours. It iterates the bodies
column family sequentially, skipping the canonical-hash lookup and random read
per block, and so covers non-canonical bodies too. Nothing found settles the
canonical question outright; a hit must be followed with the exact walk over the
range concerned, because a transaction in an orphaned block invalidates nothing.
It counts bodies it cannot decode and refuses to report a clean screen while any
remain, since a pass that silently skipped some would report a zero it has not
earned.

### `EVM-045` — are `DUPN`, `SWAPN`, `TXINDEX` reachable?

Valid opcodes from genesis until RSKIP-191 at `iris300` (#3,614,800). Class
**B**.

**Result at #3,614,799, reachability census:** `0xa8` and `0xa9` reachable in no
contract; `0xaa` surviving in three contracts, all three the same string
constant (`CROWDFUNDING_AUTH_FA…` follows it, and the 24 bytes before it are
identical across all three). 9,130 contracts, 24 MB of code, 2.94M trie nodes,
17 minutes, **no unresolved references** — a missing node would void the result,
so they are counted and a non-zero count fails the run.

A byte scan would have reported 10,525 occurrences of `0xa8`. The difference is
the decode: a byte inside a `PUSH` immediate cannot execute, because jumpdest
analysis runs over the same linear decode, so there is exactly one instruction
set per contract. The census also excludes solc's metadata trailer (CBOR map
headers are `0xa1..0xb7`, so a trailer can begin with the bytes being hunted),
dead regions after a terminator until the next `JUMPDEST`, and honours RSK's
`0xfc` code header, which moves the offset every instruction is decoded from.

**It also found the code header was never used** by any contract alive at that
height.

```sh
cargo run --release -p rustock-cli --example opcode_census -- \
    /var/lib/rustock /mnt/import/rustock-trie --block 3614799 --threads 3
```

**Widen** with `--block` (any height an archive can serve — `archive_bound`
says how high yours reaches; ours is #9,238,111) and `--ops` (any comma-separated
bytes). To cover a different era, re-run at that era's height.

**What it cannot cover:** a contract created and destroyed before the block
scanned is in no state at all, and neither is code a contract computed at
runtime. Those need the replay.

### `EVM-046` — the proposed state screen does not work

The screen was: such a call touches the account at the precompile address, a
touched account is materialised, and no ordinary path materialises a precompile
address — so an absent account would prove non-occurrence in seconds.

**Result: the premise is false.** All seven tested precompile addresses exist as
accounts in mainnet state at #9,238,111, because
`BlockExecutor.maintainPrecompiledContractStorageRoots` creates every genesis
precompile at the RSKIP-126 activation and re-creates each consensus-enabled one
at its own. The screen reports "materialised" whatever the history, and a reader
taking that as evidence toward occurrence would conclude the opposite of the
truth.

```sh
cargo run --release -p rustock-cli --example opcode_census -- \
    /var/lib/rustock /mnt/import/rustock-trie --block 9238111 \
    --account 0000000000000000000000000000000000000001,…,0000000000000000000000000000000001000016
```

A second hole would have remained even had the premise held:
`call_without_code` touches the callee only under `CallValue::Transfer`. A
zero-value transfer does touch — revm's `transfer_loaded` calls `touch_account`
before returning — so `STATICCALL` would have been covered, but `DELEGATECALL`
passes `CallValue::Apparent`, never enters that branch, and leaves no trace.

**So `EVM-046` needs the replay counter**, which is wired. It is not a deletion
candidate in any case: the spec records it active at all heights on all
networks, and it is measured to learn whether this chain holds a block an
Ethereum-derived client would get wrong.

## Instrumented, not yet run

These need one historical replay. The counters cost one relaxed atomic add on a
branch already behind a height test, so one replay measures all of them.

| Rule | Branch | Window (mainnet) |
|---|---|---|
| `BR-086` | funds-migration end age is 172,800 blocks, not 10,585 | #4,976,300–#5,468,000 |
| `EVM-024` | a bridge transaction from a privileged sender executes free | below #370,000 |
| `BR-015` | `receiveHeaders` is public rather than federation-only | #1,591,000–#3,614,800 |
| `BR-030` | an internal call to the bridge ends its caller | below #729,000 |
| `BR-011a` | a bridge method a later activation removed was invoked while it existed | below #729,000 or #1,591,000 |
| `EVM-033` | a contract created at a funded address loses that balance | below #3,614,800 |
| `EVM-034` | an internally created contract's nonce is 0, not 1 | below #1,591,000 |
| `EVM-046` | a dirty-high-bits address reaches the account, not the precompile | all heights |
| `PC-042` | a block-header depth outside `0..32,767` | all heights |

Three more have their own counters, because each carries a quantity a hit count
would lose: seven-operand `STATICCALL` (`EVM-041b`, #729,000–#1,052,700),
executions and those carrying a non-zero value; the two empty-deploy branches
below RSKIP-453 (`EVM-037`, below #7,338,024) with the largest code any internal
creation returned; and contracts deployed, with how many carried the `0xFC`
header (`EVM-032`).

```sh
# whole chain, about 13 h at two workers -- needs the node stopped
SCOPE=all BUDGET_MB=6000 bash /mnt/import/paseo/tmp/night-reexec.sh
```

**Widen or narrow the range** with `SCOPE`: `iris300` (#1..#3,620,064, ~4.5 h),
`rskip453` (#1..#7,338,024, ~10 h) or `all`. The scoping works by pre-seeding
`reexec_all`'s state file with the segments above the bound, which it skips as
already done — no code change, and the plan is printed by `--dry` before
anything executes. To inspect blocks close to a particular activation, set the
bound just above it; the segment that straddles the bound runs in full, so the
covered range always ends at a segment boundary rather than exactly at the
height.

### Two positive controls, and why the report leads with them

**A campaign that only ever returns zero has not been validated.** A counter
that cannot fire reports zero exactly as convincingly as a branch nothing took.

`BR-086`'s window is known occupied — using the ordinary end age inside it
diverged a real mainnet block — so `reexec_all` prints a verdict naming it, and
says that until it fires every other zero in the table is unproven. `EVM-024` is
the second control: it exists because the bootstrap-era federation needed it.

Read that line before any of the zeros.

## Instrumented separately: `BR-134`

The bridge sizes a single dynamic event field as `((len - 1) / 32 + 1) * 32` in
signed truncating division, so an empty field emits one zero word and the data
is 96 octets where the canonical ABI gives 64. The evidence is in the
**receipts** rather than the execution, so this needs no replay.

"Data is exactly 96 octets" is not the test: the encoding is
`offset(32) ++ length(32) ++ content` padded to whole words, so a field of 1 to
32 octets is also exactly 96 — legitimately, a 32-octet hash being the ordinary
case. The test is 96 octets **and a declared length of zero**, and the tool
reports both numbers so the gap is visible.

```sh
# whole chain, about 3.1 h at 840 blocks/s
cargo run --release -p rustock-cli --example bridge_event_width -- /var/lib/rustock
```

**Widen** with `--from` and `--to`. A probe over #1..#120,000 read 5,158 bridge
logs and found none at 96 octets.

## What cannot be removed whatever the measurement says

Worth stating, because three of the rows above are in this list and their zeros
are evidence of divergence rather than licence to delete.

- **Live at every height, so no window to close.** `EVM-046`; `PC-042` (whose
  negative half could be closed *prospectively*, with an activation, but not
  retroactively); `BR-134`, whose encoder is shared, so removing the quirk
  changes a rule rather than deleting a dead branch.
- **Needed to replay history regardless.** The lock whitelist, lifted by
  transaction at #2,475,088 — replaying any block below it needs the guard. The
  locking cap, raised past relevance rather than withdrawn; its path runs on
  every peg-in forever.
- **Rejected on their merits.** `EVM-047` — deployed contracts guard on
  `extcodesize(precompile) != 0`, so a restriction breaks working code.
  `EVM-012` — the account is already in use and the answer is disclosure.
  `EVM-060`–`EVM-065` — exercised by essentially every transaction.
- **Class C.** Seven-operand `STATICCALL`, fallback mining and every
  `orchid`/`wasabi100`-bounded branch have empty windows on testnet and regtest,
  so the code stays for mainnet; RSKIP-144/351 parallel execution has never run
  on **mainnet** but is needed on testnet.

The only free deletions in the catalogue are four class-**A** rows: `BLAKE2F`
with an empty data field, a header with no `ummRoot`, a header with no
version/edges, and a block with no execution sublists. Even then the guard keeps
one job — refusing a malformed block from a stranger — which is a decision about
what a node owes a peer rather than about replay.

## Limits of everything above

**Mainnet only.** Every result here is half the claim: several windows are empty
on testnet rather than unoccupied, and a removal needs both networks covered,
each below its own heights.

**A zero is bounded by the range that produced it.** Every tool prints the
blocks or transactions it actually read, for that reason.

**Nothing here licenses a removal by itself.** A measurement result belongs in a
proposal alongside the height ranges and the tooling; a class-A removal should
not be bundled with a class-B one, so that withdrawing one does not drag the
other; and a removal over a closed window needs no activation height, because a
node that applies it and a node that does not agree on every block either has
ever seen.
