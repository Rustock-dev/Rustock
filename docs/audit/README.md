# Audit re-triage (#46)

Re-checking the bulk RSKIP compliance review in archive `rustock-4796682`
(109 reports, 64 findings, 455 checklist assertions) against **deployed rskj**
rather than against the RSKIP text.

## Why the baseline matters

The audit states its own baseline:

> Baseline | RSKIP specification (spec is authoritative; rustock deviations are findings)

rustock's acceptance criterion is the opposite: **agreement with deployed rskj
mainnet**. Where the RSKIP and rskj disagree, rskj wins, and "fixing" rustock
to match the spec would *introduce* a fork.

Re-checking the 40 medium/low findings inverted the severity column in both
directions (see [audit-triage-medium-low.md](../audit-triage-medium-low.md)):
12 of 40 were stale spec text where acting on the recommendation would have
forked, while three findings rated LOW were real EVM divergences. A thirteenth
first filed as stale spec, FRCR-477, is a real EVM divergence too: rskj reads
RSKIP-50's code header. The same inversion is expected in the remainder — and
the first pass found it again.

## Verdict vocabulary

| verdict | meaning |
|---|---|
| **Divergence from rskj** | rustock differs from deployed rskj. Real. |
| **Spec is stale** | rustock matches rskj; the RSKIP text is wrong or outdated. No action. |
| **No consensus effect** | local-only getter, RPC formatting, mempool policy. |
| **Premise does not hold** | the finding describes code that is not there, or is there and correct. |
| **Closed window, unexercised** | a genuine divergence in a permanently closed activation window, proven unexercised by the whole-chain replay. |

## Where the audit stands

| scope | status |
|---|---|
| 7 high findings (6 distinct defects) | done — #43, [audit-response-2026-09.md](../audit-response-2026-09.md) |
| 15 medium + 25 low | done — [audit-triage-medium-low.md](../audit-triage-medium-low.md), #43 and #44 |
| **17 info findings** | **done — this directory** |
| **348 checklist rows marked OK** | **done another way** — every rule they check was read in rskj's own code, [below](#the-70-rskips-read-against-rskj) |
| **111 RSKIPs the audit never reviewed** | **done for the 11 Adopted or Accepted** — #149, [below](#rskips-the-audit-skipped-149) |

## The 348 OK rows

The findings tables were triaged. Each report also carries a **compliance
checklist** whose rows are marked OK, and nobody has re-checked those. A row
marked OK was checked against the RSKIP text, which is not our acceptance
criterion — so an OK row against a stale RSKIP can hide a divergence from rskj,
which is the direction that matters.

Order of work is by **blast radius**, not by the audit's severity column:

1. Anything transaction-callable on the Bridge.
2. EVM opcode availability and gas, per activation.
3. Block/header validity rules.
4. Local-only getters last — wrong RPC answers cannot fork.

They were not re-checked row by row. Every RSKIP rskj runs has since been read in
rskj's own code beside rustock's, which is the criterion these rows lacked: the 70
that rsk.zone lists in [the section below](#the-70-rskips-read-against-rskj), and the
rest one by one before them, #149's eleven and 26 among them. A row that checked a
rule against a stale RSKIP is answered by its rule's verdict there.

## Cluster files

| file | findings |
|---|---|
| [evm.md](evm.md) | FRCR-279, -281, -577; RSKIP-131, -191 |
| [storage-state.md](storage-state.md) | FRCR-373, -375, -377, -379; RSKIP-144 |
| [rewards-fees.md](rewards-fees.md) | FRCR-876 |
| [bridge.md](bridge.md) | FRCR-73; RSKIP-179, -502 |
| [addresses-accounts.md](addresses-accounts.md) | FRCR-774 |
| [misc.md](misc.md) | FRCR-173, -174, -175, -176, -177, -374, -576; RSKIP-0, -59 |
| [mining-consensus.md](mining-consensus.md) | RSKIP-46, -157 |
| [standards.md](standards.md) | RSKIP-148, -159 |

## Result of the info pass

17 findings. **One was not info.**

| verdict | count |
|---|---:|
| Spec is stale / observation only | 9 |
| Premise does not hold | 3 |
| No consensus effect | 3 |
| Absent from rustock *and* from mainnet consensus | 1 |
| **Divergence from rskj** | **1 — FRCR-279** |

[FRCR-279](evm.md#frcr-279) was filed INFO as "no distinct RSKIP169 activation
gate". It is a real EVM divergence across a ~1.22M-block window, in a
permanently closed range that the whole-chain replay proves unexercised. The
severity column was wrong by three levels. It is now fixed, with RSKIP-131
below.

Archive: `/root/doc/rustock-4796682.zip`.

## RSKIPs the audit skipped (#149)

The audit indexed the 220 RSKIPs of the RSKIPs repository's index at
`5736b2c` and reviewed 109. #149 sorted the 111 it skipped by their status
in that index (80 Draft, 6 Rejected, 14 without a spec file, 4 Accepted,
7 Adopted) and named the 11 Adopted or Accepted as the ones to check. Each
was read in rskj as deployed, `VETIVER-9.0.4` (`master` at `730c235ad`
changes none of the code involved), and in rustock:

| RSKIP | title | file | verdict |
|---|---|---|---|
| 131 | CREATE2 after SUICIDE in the same block | [evm.md](evm.md#rskip-131--create2-after-suicide-in-the-same-block) | **divergence from rskj — fixed** |
| 191 | DUPN, SWAPN and TXINDEX removed | [evm.md](evm.md#rskip-191--dupn-swapn-and-txindex-removed) | **divergence, closed window — fixed** |
| 179 | BTC-RSK timestamp linking | [bridge.md](bridge.md#rskip-179--btc-rsk-timestamp-linking) | matches rskj |
| 502 | PowPeg and Union Bridge integration | [bridge.md](bridge.md#rskip-502--powpeg-and-union-bridge-integration) | **divergence, live on mainnet — fixed** |
| 46 | mining fees in the header | [mining-consensus.md](mining-consensus.md) | matches rskj |
| 157 | cumulative difficulty in JSON-RPC | [mining-consensus.md](mining-consensus.md) | no consensus effect; matches rskj |
| 148 | ERC1820 registry | [standards.md](standards.md) | premise does not hold |
| 159 | minimal proxy contract | [standards.md](standards.md) | premise does not hold |
| 144 | parallel transaction execution | [storage-state.md](storage-state.md#rskip-144--parallel-transaction-execution) | matches rskj; live on testnet |
| 0 | RSKIP purpose and guidelines | [misc.md](misc.md) | no consensus effect |
| 59 | child contracts | [misc.md](misc.md) | absent from rskj and rustock |

Three of the eleven were real divergences, and #149's spot check had the
first one backwards: the test it cited as covering RSKIP-131 asserted the
creation rskj refuses. The one that mattered most was RSKIP-502: an
upgrade-order gate made seven Bridge methods callable on mainnet from
#8,804,200, where rskj has none of them, so one transaction calling one would
have split rustock from the chain. RSKIP-144 is not the closed window #149
took it for: testnet runs it.

### What sorting by status leaves out

A rule is consensus when rskj runs it, whatever its RSKIP's status. Of the 97
RSKIP rules rskj's `reference.conf` schedules (its 98th rule,
`areBridgeTxsPaid`, has no RSKIP, and rustock schedules it at rskj's heights),
the index at `5736b2c`, which #149 sorted by, lists 13 as Draft (RSKIP-110,
-119, -122, -134, -152, -351, -529, -535, -536, -540, -544, -551, -552) and
does not list 13 at all (RSKIP-88, -90, -97, -103, -124, -126, -136, -137,
-143, -150, -151, -156, -559). The 13 Drafts fall in the class #149 set aside
as not consensus, and the 13 unlisted were outside the audit altogether. The
index has moved since: as it stands now (last changed 16 October), it has seven
of those Drafts as Adopted (RSKIP-119, -152, -536, -540, -544, -551, -552),
three as Adopted (Testnet) (RSKIP-351, -529, -535), and RSKIP-559 as Draft. The
index at `5736b2c` also disagrees with the RSKIPs' own files: it has RSKIP-131,
-179 and -191 as Adopted where their files say Draft, and RSKIP-59 as Accepted
where its file says Rejected.

None of those 26 is among the eleven, and the chain can't settle them: every
block of both networks has been re-run and matches its header, which shows a
rule matches wherever a real block exercised it, but says nothing of a case no
block reached, or of a block rskj refuses, since the chain holds none. So each
was read where rskj reads it, at `VETIVER-9.0.4`, against rustock. Five
differed:

| RSKIP | rskj | rustock did | fix, and its test |
|---|---|---|---|
| 90 | Before orchid (mainnet #729,000), a precompile that returns leaves the caller's return data as it was (`Program.executePrecompiled`). | Made the output the return data. | `RskHandler::with_rskip90`; `test_precompile_calls_and_the_return_data_by_height` |
| 103 | Before orchid060 (mainnet #729,000 to #1,052,699), STATICCALL takes a value as CALL does (`VM.calculateCallValue`): seven arguments, `VT_CALL` and the stipend for a value, which moves to a callee that runs static. | Took six arguments and no value. | `rsk_static_call_with_value`; `test_pre_rskip103_staticcall_takes_a_value` |
| 134 | `increaseLockingCap` fails the call where the new cap times its multiplier passes a `long` (`Coin.multiply`). | Saturated the product and went on. | `governance.rs`; `a_cap_whose_limit_passes_a_long_fails_the_increase` |
| 136 | Before bahamas (mainnet #3,397, or wherever `rskip136` is moved), a transaction's own call to any precompile is held to its limit by the precompile's price alone, and pays that price on top of the intrinsic cost, past the limit if it comes to that (`TransactionExecutor.call`). | Did so for the Bridge only. | the executor's direct-call path; `before_rskip136_a_direct_precompile_call_is_held_to_its_limit_by_its_price_alone`, and the same for a native contract |
| 150 | Before twoToThree (mainnet #2,018,000, testnet #504,000), a call's stipend is added to the gas it asks for as a plain `long` (`VM.getMessageCall`): at 2^63 - 2,300 or more the sum goes below zero, and the calling frame fails with all its gas. | Saturated the sum. | `rsk_callee_gas`; `test_pre_rskip150_stipend_sum_past_a_long_fails_the_caller` |

Four of the five sit in windows the chain has passed, where no block reached
the case, and RSKIP-134's needs a cap no authorizer has asked for.

RSKIP-136 runs through the path for the calls rskj makes outside its VM, which
also takes the federation's free Bridge transactions. Three more differences
in that path are fixed with it:

- It held a transaction to none of `TransactionExecutor.init`: its sender's
  next nonce, a limit that pays its intrinsic cost, a balance that covers its
  value and its whole limit. revm holds every other transaction to these.
  `test_bridge_tx_gas0_fails_for_unknown_sender` asserted a block that `init`
  refuses, which `TransactionListExecutor` stops at; it now asserts the
  refusal. A free Bridge transaction meets this on rskj's regtest at any
  height, where the federators' Bridge transactions stay free.
- It priced a byte of data at 68 gas where RSKIP400 makes it 16.
- Where a block runs as its sublists, it paid each fee to REMASC at once, and
  REMASC was paid it again with the block's other fees.

The last two, and RSKIP-136's own difference, can't happen on mainnet or
testnet: mainnet's window is its first 3,397 blocks, which rustock replays,
and testnet schedules bahamas at 0. They can on a chain that moves `rskip136`.

The other 21 match:

| RSKIP | what rskj gates on it |
|---|---|
| 88 | the Bridge refusing calls from contracts, and how it ends a call it can't read |
| 97 | the difficulty's ten-minute rule and its minimum |
| 110 | the fork-detection data in a merged-mining coinbase |
| 119 | the BlockHeader precompile |
| 122 | the Bridge's `getBtcTransactionConfirmations`, its price included |
| 124 | the Bridge's `receiveHeaders`: its price, who may call it, a header's size |
| 126 | the state root a header carries, and how a block's roots are checked |
| 137 | the altBN128 precompiles, and their answer to a point off the curve before RSKIP197 |
| 143 | the Bridge's coinbase-information methods |
| 151 | SELFBALANCE |
| 152 | CHAINID |
| 156 | the difficulty's bound divisor, 400 from it except on regtest |
| 351 | a block's sublist gas limits and their header field |
| 529 | the Union Bridge's event methods |
| 535 | the header's version 2, and the Union Bridge's base event it carries |
| 536 | the BlockHeader precompile's `getCumulativeWork` and `getDifficultyWithUncles` |
| 540 | the Bridge's `getEstimatedFeesForPegOutAmount` |
| 544 | refusing new code that starts with 0xEF |
| 551 | where a rejected peg-in stops being marked as processed |
| 552 | BLAKE2F's price for a call with no data |
| 559 | sorting the pegouts; no network schedules it, and `tbd1000`, its upgrade, is accepted and reported as changing nothing |

18 of the 26 are also among the rules
`every_rule_activates_where_rskjs_config_puts_it` holds to rskj's heights.

## The 70 RSKIPs, read against rskj

Every RSKIP that rsk.zone lists as activated, 70 of them, was read side by side in rskj's release `VETIVER-9.0.4` and in rustock: the code of each in both clients, with rskj's own tests of it as the checklist. So was each statement rsk.zone makes about what a node does: merge mining, the PowPeg, REMASC, the EVM and its precompiles, the state tree, the chain IDs. Where rustock did otherwise than rskj it now does as rskj does, and each fix comes with a test that fails without it. 51 of the 70 matched rskj as they were; 19 had differences, all fixed. rskj then confirmed the fixes on its own regtest: for each fix a regtest can reach, a case builds blocks on rskj's release, the final build imports them with every state root checked against its header and agrees, and a build from before the fix parts from rskj where the fix says it would. The rest say why no regtest reaches them.

rskj's tests of these rules, 1,239 of them, each have a verdict in `docs/rskj-tests/`: 925 ported, 206 held by a test rustock had, 94 that cannot arise in rustock, and 13 whose port found rustock doing otherwise, now fixed; one more is in rskj's `master` and not in the release ([`docs/rskj-test-parity.md`](../rskj-test-parity.md) has the summary).

### Before any upgrade (from block 0)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 15 | REMASC: a block's fees paid to its miner 4,000 blocks later, shared with its siblings | Three fixes. REMASC pays the matured block of the running block's own chain, and its siblings, in rskj's order; rustock took them from the main chain by number, which shows only on a block run beside the chain (confirmed on rskj's regtest). Its payment to the federation takes a new federation's activation age at the running block's upgrades, not the paid block's (confirmed on rskj's regtest). A block or body the store lacks, when REMASC reads it, is the node's fault and no rule's; rustock refused the block (no regtest reaches it: a store fault). |
| 40 | The Bridge: peg-ins, the federation and its changes, peg-outs and their signatures | Fixed in each of its three parts. **Bitcoin headers:** before RSKIP-88, `receiveHeaders` fails for a header that counts its transactions below zero (no regtest reaches it: before the first upgrades, where rskj's regtest cannot start); a Bitcoin block looked up by hash is read from the Bridge's storage alone, not from a node-wide cache that keeps other chains' blocks (confirmed on rskj's regtest). **The federation:** with no federation pending, a vote to add a member whose key is no point answers -10 (confirmed on rskj's regtest); a chain from rskj's regtest genesis has its five authorizers, three deciding, and its ten free Bridge senders (confirmed on rskj's regtest); a member key of the one byte `00` is kept, as rskj keeps it (confirmed on rskj's regtest); a migration halves the balance with no floor and, under dust, fails inside the migration age and retires the federation past it (confirmed on rskj's regtest); and it pays the active federation as stored, in its own format (confirmed on rskj's regtest). **Peg-outs:** a signature is read as bitcoinj reads it with Spongy Castle 1.58, every element of it; a high s signs nothing; the input gets the DER bitcoinj writes (confirmed on rskj's regtest). The confirmed peg-out taken is the first in the walk order of the set the call read (confirmed on rskj's regtest). `addSignature` writes back the set it read (confirmed on rskj's regtest), logs the member's key compressed (confirmed on rskj's regtest), and fails where an input has no redeem script (no regtest reaches it: the Bridge builds every transaction it signs). The federations' UTXOs are written back where rskj reads them: by the fee estimate and by a peg-in turned down (confirmed on rskj's regtest). Before RSKIP-88, a call answered with null saves nothing, and the state getters write back what they read (no regtest reaches it: before the first upgrades). |
| 42 | Receipts with no intermediate state root | Matches rskj. |
| 107 | The unitrie's smaller nodes, as stored and hashed | Fixed: a tree node with bytes after its long value's length is refused (no regtest reaches it: no client writes one; a peer could send one in a snapshot). |
| 225 | The emergency keys of the federation's time-locked script | Matches rskj. |

### Orchid (mainnet #729,000, testnet #0)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 85 | REMASC holds back a reward under the minimum payable | Fixed: REMASC holds back a reward under the block's whole minimum gas price times its minimum payable gas; rustock stopped the price at 2^64 - 1 (confirmed on rskj's regtest, at a price of 2^65). |
| 87 | The lock whitelist's unlimited mode | Matches rskj. |
| 89 | The Bridge's Bitcoin block query methods | Matches rskj. |
| 91 | STATICCALL | Matches rskj. |
| 92 | The merged-mining merkle proof's encoding | Matches rskj. |
| 98 | No federated fallback mining | Matches rskj. |

### Wasabi (mainnet #1,591,000, testnet #0)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 106 | The HD wallet precompile | Fixed: a derivation whose HMAC's left half equals the curve's order keeps its parent's point; rustock threw (no regtest reaches it: it needs such an HMAC output). |
| 120 | The shift opcodes | Matches rskj. |
| 123 | Federation members with several keys | Fixed: the new federation is written back after any successful call that read it (no regtest reaches it: before Wasabi, where rskj's regtest cannot start). |
| 125 | CREATE2 | Matches rskj. |
| 132 | A higher gas cost for `receiveHeaders` | Matches rskj. |

### Papyrus (mainnet #2,392,700, testnet #863,000)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 140 | EXTCODEHASH | Matches rskj. |
| 146 | The Bridge's events in Solidity's encoding | Fixed: the release request queue is written back, both its cells, after every method that read it (confirmed on rskj's regtest); `getStateForDebugging` leaves out the entries of the other encoding (confirmed on rskj's regtest). |

### Iris (mainnet #3,614,800, testnet #2,060,500)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 153 | The BLAKE2 compression precompile | Matches rskj. |
| 169 | EXTCODEHASH rectified | Fixed: before it, EXTCODEHASH of a contract whose empty code a creation of the same block saved answers keccak256 of nothing; rustock answered 0 (confirmed on rskj's regtest). |
| 170 | Peg-ins to any address | Fixed: a peg-in with an output bitcoinj can't parse is refunded; rustock credited it (confirmed on rskj's regtest). |
| 171 | The return buffer cleared by the call opcodes | Matches rskj. |
| 174 | A creation keeps the balance at its address | Fixed: before it, a deployment that fails leaves the balance at its address as it was (confirmed on rskj's regtest); local calls, traces and estimates apply it as a block's run does (confirmed on rskj's regtest). |
| 176 | Flyover: peg-ins to a programmable address | Fixed: before RSKIP-293, a Flyover payment with an output that is no address answers GENERIC_ERROR; rustock answered -304, or registered it (confirmed on rskj's regtest). |
| 181 | Peg-in rejection events | Matches rskj. |
| 185 | Peg-out refunds and events | Fixed: before it, a contract's `releaseBtc` fails the contract that made the call, with all its gas; rustock failed only the call (confirmed on rskj's regtest). |
| 186 | The active federation's creation block height | Fixed: the getter answers the next federation's creation block once it is active, before `updateCollections` moves it (confirmed on rskj's regtest). |
| 197 | Precompile calls with CALL's semantics | Matches rskj; its window with RSKIP-185 is under that one. |
| 199 | `registerBtcTransaction` open to anyone | Matches rskj. |
| 200 | Limits on `receiveHeaders` | Matches rskj. |
| 201 | The federation's time-locked emergency script | Fixed: below the peg-out index, a registration is sorted by running each input's script as bitcoinj does, signatures checked; rustock compared the hash of the redeem script an input carried (confirmed on rskj's regtest). |
| 218 | A new fee address for the Core Developers Fund | Matches rskj. |
| 219 | New minimum values for peg-ins and peg-outs | Matches rskj. |
| 220 | A Bitcoin block or header by hash or height in the Bridge's methods | Matches rskj. |

### Hop (mainnet #4,598,500, testnet #3,103,000)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 271 | Peg-outs in batches | Fixed: a request must cover the fee of a peg-out sized by the active federation's format (confirmed on rskj's regtest); a batch is halved while it is over 100,000 bytes (no regtest reaches it: it needs hundreds of peg-ins); a batched request with no hash fails the call (confirmed on rskj's regtest); the next peg-out height is written back (confirmed on rskj's regtest). |
| 284 | Flyover refund addresses read anew | Matches rskj. |
| 290 | Testnet's minimum difficulty | Matches rskj. |
| 293 | `getActivePowpegRedeemScript`, and more Flyover checks | Matches rskj. |
| 294 | At most so many inputs in a migration | Matches rskj. |
| 297 | A wider Bitcoin-to-RSK timestamp gap on testnet | Matches rskj. |

### Hop 4.0.1 (mainnet #4,976,300, testnet #3,362,200)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 353 | Redeem scripts held to Bitcoin Core's standard checks | Matches rskj. |
| 357 | The confirmations of a migration period adjusted | Matches rskj. |

### Fingerroot (mainnet #5,468,000, testnet #4,015,800)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 252 | The transaction gas price cap | Matches rskj. |
| 326 | Peg-out events improved | Matches rskj. |
| 374 | The confirmations of a migration period restored | Matches rskj. |
| 375 | Peg-outs waiting for signatures keyed by their creation transaction's hash | Fixed: a confirmed peg-out under a key the set waiting for signatures already holds, or under none, fails `updateCollections`; rustock replaced the entry, or used the update's own hash (confirmed on rskj's regtest, both). |
| 377 | The last retired federation's standard P2SH script kept | Matches rskj. |
| 383 | A longer PowPeg activation age | Matches rskj. |
| 385 | `getEstimatedFeesForNextPegOutEvent` improved | Matches rskj. |

### Arrowhead (mainnet #6,223,700, testnet #4,927,100)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 203 | The `getCallStackDepth` precompile method | Matches rskj. |
| 376 | Migration transactions of version 2 | Matches rskj. |
| 379 | An index of peg-out and migration transactions | Matches rskj. |
| 398 | PUSH0 | Matches rskj. |
| 400 | Cheaper calldata | Matches rskj. |
| 412 | BASEFEE | Matches rskj. |
| 415 | Pegnatories' addresses derived rightly from their keys | Matches rskj. |
| 417 | A Bridge call from a contract that answers an error no longer reverts it | Matches rskj. |

### Arrowhead 6.3.1 (mainnet #6,549,300, not on testnet)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 434 | Bitcoin chain work of up to 12 unsigned bytes | Matches rskj. |

### Lovell (mainnet #7,338,024, testnet #6,110,487)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 419 | The PowPeg's spendability validation protocol (SVP) | Fixed: `createFederation` answers -4 while a proposed federation awaits its validation (confirmed on rskj's regtest); a call to sign the SVP spend transaction with a key of no proposed member fails, and one with a member's key in its hybrid form signs (confirmed on rskj's regtest). |
| 427 | Peg-out events' amounts in wei | Matches rskj. |
| 428 | A peg-out creation event with its UTXOs' values | Matches rskj. |
| 438 | Initcode's size limit and its gas per word | Matches rskj. |
| 445 | MCOPY | Matches rskj. |
| 446 | Transient storage | Matches rskj. |
| 453 | No account made by a failed CREATE or CREATE2 | Matches rskj. |
| 454 | Bitcoin chain work of up to 32 unsigned bytes | Fixed: before it, a checkpoint whose chain work is past 12 bytes is refused, as bitcoinj refuses to write it; rustock kept its low 12 bytes (no regtest reaches it: rskj's regtest seeds no checkpoint). |
| 459 | Rejected peg-ins marked processed | Matches rskj. |
| 460 | Non-standard outputs skipped in finding the witness commitment | Matches rskj. |

### Reed (mainnet #8,052,200, testnet #6,835,700)

| RSKIP | What it makes a node do | What was found |
|---|---|---|
| 305 | Segwit peg-outs | Fixed: a segwit input is signed only over the value kept for it, and only from RSKIP-305; before it, or with none kept, the call fails; rustock signed over a value of 0 (confirmed on rskj's regtest). |
| 516 | Addition and multiplication on secp256k1 as precompiles | Matches rskj. |

### Rules beyond the 70

| Where | What was found |
|---|---|
| The EVM | A 32-byte word with bits set above an address's 20 bytes names no precompile, for a call, EXTCODESIZE or EXTCODEHASH (confirmed on rskj's regtest). Stored code that starts `0xEF01` is code like any other, not a pointer (confirmed on rskj's regtest). A transaction from an address that holds code runs (no regtest reaches it: it needs a key for such an address). BLOCKHASH answers from the running block's own ancestors, so a block beside the chain reads its own branch (confirmed on rskj's regtest). |
| Block 0 and the genesis file | Block 0 hashes over its difficulty as its file writes it, and a zero coinbase or gas limit as rskj hashes them (confirmed on rskj's regtest). A genesis file is read as rskj reads it: a timestamp or gas limit over 8 bytes, or no minimum gas price, refused (confirmed on rskj's regtest). Regtest keeps its difficulty divisor of 2,048 under RSKIP-156 (confirmed on rskj's regtest). |
| The Bridge, beyond the 70 | The witness root registered for a Bitcoin block is read only from RSKIP-143 (no regtest reaches it: a coinbase is registered only from RSKIP-143). A free Bridge transaction's receipt keeps what its Bridge call logged before it threw (found on rskj's regtest, which shows it). |
| The miner | A solved block a rule refuses is answered `INVALID_BLOCK` (confirmed on rskj's regtest). A solution of work whose parent is no longer the head is connected as any block (confirmed on rskj's regtest). Uncles are picked by the rule a block meets on import, and a block it solves is held to every rule, its fork detection data included; the gas limit steers toward rskj's `targetgaslimit`, 6,800,000 or a regtest's 7,500,000, by `forcegaslimit` or by the gas the parent used, with rskj's `GasLimitCalculator` tests ported; a gas limit target under the minimum is the minimum; the minimum gas price steers toward rskj's `miner.minGasPrice`, 0 unless set, or with `miner.stableGasPrice` on toward a price held stable in a currency, fetched from a web API's JSON (read as rskj's Jackson reads it) or a contract's answer to a call on the node, with every rskj test of `co.rsk.mine.gas` and `co.rsk.config.mining` ported (both sources run on a regtest node here, its RPC off for the call); a solved block the node fails to store is answered `INVALID_BLOCK` too, and one whose parent the store lacks `NO_PARENT`; a timed miner started again right after a stop runs one thread; `evm_increaseTime` ignores zero or fewer seconds (no regtest reaches it: what a node mines is its own). |
| Sync and relay | A node on a new database runs block 1 in follow mode as it arrives (found on rskj's regtest, which shows it). A block a peer sends whole, timed no later than its parent, is not relayed. Snap sync holds its blocks to the rules rskj's snap client runs. An old store's block 0 is written again (no regtest reaches it: what a node relays and keeps). |
