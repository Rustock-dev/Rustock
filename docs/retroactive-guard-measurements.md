# Measuring whether a guard can be removed

A guard over a window that can never reopen is a candidate for deletion. The
condition is not that the window is closed — it is that **nothing in the window
ever took the branch**, which is a claim about this chain's history and not
about the code. Removing a guard on reasoning alone changes the state root this
node computes for blocks that are already on the chain.

This document records what has been measured, with the exact command for each
and how to widen it.

As of 2026-11-23 the whole chain has been re-executed with every counter live:
**9,429,718 blocks, 106 segments, 17.4 hours**, both positive controls firing.
Twenty-eight branches were counted, thirteen of them never taken. What remains
unrun is named as such -- `BR-134`, which needs a receipts scan rather than a
replay, and every result's testnet half.

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

**And the replay disagreed with it, which is the useful part.** The whole-chain
run found `SWAPN` **executed once**, in a chain where the census found it
reachable in nothing. Both results are correct, and the gap between them is the
census's scope: it decodes the contracts **alive at one height**, so it cannot
see a contract that ran and was destroyed before that height, and it cannot see
constructor code, which executes and is never stored.

**Located, and then settled.** A second run carrying the block number and the
executing contract put the occurrence at mainnet **#996,053**, transaction 0,
contract `0x56bF637ee636D8ebDE45a046c45DEac6eF67a019`.

The transaction is a contract **creation** whose init code is 68 octets of
`transfer(address,uint256)` calldata:

```text
a9059cbb 000…c3439c113bf4e8da22f5bc6093a0dd7b95de06a4 000…1000000000000000000
```

Someone sent an ERC-20 transfer with an empty `to`, so the node treated the
calldata as init code and executed it. Its first octet is `0xa9`, which below
iris300 is `SWAPN`, and the octet after it is the operand: `SWAPN 5` on an empty
stack. The creation fails, and the address holds no account to this day. Nobody
ever used the opcode deliberately.

**And removing it changes nothing.** With `SWAPN` present, `0xa9` underflows --
an exceptional halt consuming all gas. With it replaced by the invalid-opcode
handler, `0xa9` is `OpcodeNotFound` -- also an exceptional halt consuming all
gas. Replaying the real block against the real pre-state both ways gives
identical results:

| | `SWAPN` enabled | `SWAPN` disabled |
|---|---|---|
| state root | `0x11686e23…91664e` | `0x11686e23…91664e` |
| block gas used | 6,800,000 | 6,800,000 |
| paid fees | 402,832,000,000,000 | 402,832,000,000,000 |
| receipt 0 | gas 6,800,000, status false | gas 6,800,000, status false |
| receipt 1 (REMASC) | gas 0, status true | gas 0, status true |

Both roots match the baseline. **So `EVM-045` is removable on mainnet despite
its count being non-zero** -- the window is occupied, but only nominally, and no
count alone could have shown that.

The fixture is `crates/execution/tests/swapn_at_996053.rs`, which replays the
block from the baseline seed and the archival trie. Run it once as it stands and
once with `SWAPN` replaced by `invalid_opcode` in `rsk_instructions::install`;
one instruction changes between the runs and nothing else. Moving `iris300`
instead would also move RSKIP-218's REMASC recipient and RSKIP-174's
created-at-a-funded-address rule, either of which would shift the root for
reasons unrelated to `SWAPN`.

**Still open:** testnet. Its activation ladder differs and no equivalent replay
has been run, so this closes the mainnet half only. `DUPN` and `TXINDEX` never
executed on mainnet at all.

So the census's zero was never the claim the campaign needed. It is a sound
*necessary condition* over surviving code and nothing more -- which is how it was
written up here, and it is still the cheapest way to rule a rule out. But a
negative from it does not close a window; only the replay does. `EVM-041a`'s
shift-opcode half, which this report hands to the census for want of a branch to
count, inherits exactly that limitation and should be read with it.

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

## What the whole-chain run measured

Run over the whole chain on 2026-11-22/23: **9,429,718 blocks, 106 segments,
17.4 hours** at two workers, with an archival trie behind the segment set as a
fallback. Every block's state root was verified, and every segment with it.

The counters cost one relaxed atomic add on a branch already behind a height
test, so this one replay measured every row below at once.

### Read the controls first

| Control | Count | |
|---|---|---|
| `BR-086` | **55,834** | fired |
| `EVM-023a` | **4,875** | fired |

Both fired, so the zeros below are measurements rather than silence. A counter
that cannot fire reports zero exactly as convincingly as a branch nothing took,
which is why the tool prints this verdict before the table and refuses to call
the campaign validated without it.

### Windows this chain never entered

| Rule | Branch | Window (mainnet) |
|---|---|---|
| `BR-030` | an internal call to the bridge ends its caller | below #729,000 |
| `EVM-041a` | `STATICCALL` reached before RSKIP-91 made it an opcode | below #729,000 |
| `EVM-062` | a call's stipend sum overflowed a signed 64-bit integer | below #2,018,000 |
| `EVM-043` | `EXTCODEHASH` answered from the trie-existence marker | #2,392,700–#3,614,800 |
| `EVM-023` | a precompile's exception burned the calling frame's gas | below #3,614,800 |
| `BR-097` | a Bitcoin header refused for 12-octet chain-work overflow | BTC #849,138 to #6,549,300 |
| `EVM-046` | a dirty-high-bits address reached the account, not the precompile | all heights |
| `PC-042` | a block-header depth outside `0..32,767` | all heights |
| `PC-051` | `ECRECOVER` given a `v` word with information above octet 63 | all heights |
| `BV-110` | a gas limit of 2^63 or more, read as a negative signed long | all heights |
| `BV-125` | the uncle count's 32-bit product wrapped in the difficulty | all heights |
| `BV-173` | the uncle sum wrapped across the 32 newest headers | all heights |
| `BR-061` | a peg-out amount that did not fit a signed 64-bit long | all heights |

**Every narrowing guard reports zero except `BR-112`.** No overflow or
truncation has ever changed a value this chain accepted. That is the baseline
those counters now monitor against, and it is clean.

### Windows this chain occupies

| Rule | Branch | Count |
|---|---|---|
| `EVM-100` | a precompile wrote its whole output past the declared region | **352,910** |
| `EVM-101` (case 2) | a call that ran nothing kept the caller's return-data buffer | **178,728** |
| `BR-015` | `receiveHeaders` public rather than federation-only | 84,315 |
| `BR-086` | funds-migration end age 172,800 rather than 10,585 | 55,834 |
| `EVM-024` | a privileged sender's bridge transaction executed free | 44,646 |
| `BR-136` | `addSignature`'s topic carried the federator's BTC key | 28,038 |
| `EVM-033` | a contract created at a funded address lost that balance | 4,874 |
| `EVM-023a` | a precompile transaction charged past its own gas limit | 4,875 |
| `EVM-034` | an internally created contract's nonce was 0, not 1 | 562 |
| `BR-011a` | a windowed bridge method invoked while it existed | 83 |
| `EVM-101` (case 3) | a refused `CREATE` kept a stale return-data buffer | 43 |
| `BR-125` | `commitFederation` handed over immediately, with no SVP | 13 |
| `BR-055` | a rejected, unrefundable peg-in marked processed | 4 |
| `BR-112` | a negative flyover code sign-extended across 256 bits | 3 |
| `EVM-101` (case 1) | a successful precompile call kept the caller's buffer | 11 |

### The quantities a hit count would have lost

- **RSK's own opcodes** (`EVM-045`): `DUPN` 0, **`SWAPN` 1**, `TXINDEX` 0. The
  one occurrence is mainnet #996,053, and replaying that block with the opcode
  removed gives an identical state root -- see `EVM-045` above. Occupied, but
  removable.
- **The code header** (`EVM-032`): 15,066 contracts deployed, **none** carrying
  the `0xFC` header. This answers the question the census could only narrow.
- **Seven-operand `STATICCALL`** (`EVM-041b`): 0 executed, 0 with a non-zero
  value, over the whole #729,000–#1,052,700 window.
- **Empty deploys below RSKIP-453** (`EVM-037`): 0 over the maximum code size,
  **172** unable to pay the code deposit; the largest code any of them returned
  was 24,532 octets, 44 short of the limit.

### High-water marks

Maxima rather than counts, so a zero here means nothing was observed at all
rather than that a branch was never taken.

| | Value | Block |
|---|---|---|
| Deepest CALL-family frame | **401** | #7,931,702 |
| Most EVM memory expanded | **422,944 octets** | #9,085,105 |
| Largest `RETURN` payload | **24,573 octets** | #6,714,553 |

Two of the three sit on limits. 401 is one past RSKIP-150's call-depth cap of
400 — the mark is taken where the frame is *opened*, before the cap refuses it,
so it records the depth reached rather than the depth that survived. And 24,573
is three octets under the 24,576 contract-size ceiling, so the largest `RETURN`
this chain ever carried is a maximum-size deployment.

### Four zeros that prove less than they look

Stated here rather than left for a reader to infer:

- **`EVM-023` is known occupied on testnet**, at #808,484. Its mainnet zero says
  nothing about the branch being dead; a testnet replay would settle it and none
  has been run.
- **`BR-061` is unreachable on mainnet and testnet balances by construction** —
  no balance approaches 2^63 satoshis. Its zero was never in doubt. The branch
  *is* reachable on regtest, whose genesis funds accounts with 10^30 wei.
- **`EVM-041a` is half-measured.** `STATICCALL` below orchid is an explicit
  invalid-opcode substitution and is counted. The shift opcodes and `CREATE2`
  below wasabi100 come from the spec selection rather than from a branch, so
  there is nothing at that site to count; they need the reachability census.
- **`BV-110`'s zero is expected.** A header carrying a gas limit of 2^63 or more
  is invalid, so no canonical block holds one. A non-zero there would have been
  a finding about the decoder, not about the chain.

### One figure not yet reconciled

`EVM-023a` reports **4,875** occurrences below #3,397, but both the
specification and the RSKIP-136 write-up describe mainnet #3,307 as *the*
occurrence, singular. 4,875 across 3,396 blocks is 1.4 per block, which is
plausible for early federation traffic but contradicts the written claim. The
control *fired*, which is all that was needed to validate the campaign, but the
figure should not be cited as evidence until the discrepancy is resolved.

### Segment completeness, as a by-product

The replay reads state through the segment set with an archival trie behind it,
and records every read a segment could not serve. Over the whole chain:
**510,219 such reads, none absent from both stores**, concentrated in a small
number of blocks that perform deep Bridge walks — a peg-in's confirmation count
or a pegout's UTXO selection.

That list is the input to repairing the segments. An earlier run capped the
record at 50,000 keys and so retained 9.8% of it; the list is now appended per
finished segment, which both removes the cap in practice and makes it survive an
interrupted run.

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
