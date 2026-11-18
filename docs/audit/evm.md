# evm — info findings

## FRCR-279 — no distinct RSKIP169 activation gate on EXTCODEHASH

**Verdict: divergence from rskj — closed window, unexercised.** Filed INFO;
it is not info.

**Since fixed,** with RSKIP-131 below: before RSKIP169 rustock now answers
`ZERO_HASH` for a contract whose code key holds nothing, a contract being an
account that carries the marker rskj's `setupContract` writes, on testnet's
schedule as on mainnet's (`rsk_extcodehash`; test
`extcodehash_of_a_contract_without_code_is_0_before_rskip169`). The analysis
below is as it was written before the fix.

### What rskj does

`VM.doEXTCODEHASH` calls `program.getCodeHashAt(address, activations.isActive(RSKIP169))`,
and `RSKIP169` activates at **iris300** (`reference.conf:43`). The two branches
are `MutableRepository.getCodeHashStandard` and `getCodeHashNonStandard`, and
they differ in exactly one case:

| account state | pre-RSKIP169 (`NonStandard`) | post-RSKIP169 (`Standard`) |
|---|---|---|
| does not exist | `ZERO_HASH` | `ZERO_HASH` |
| exists, not a contract | `keccak256("")` | `keccak256("")` |
| **exists, is a contract, code key has no value** | **`ZERO_HASH`** | **`keccak256("")`** |

rskj comments the old branch itself: *"Returning ZERO_HASH is the non standard
implementation we had pre RSKIP169 implementation and thus me must honor it."*

### What rustock does

`rsk_extcodehash` (`crates/execution/src/rsk_instructions.rs`) has **no gate**:

```rust
let result = if load.is_empty {
    B256::ZERO
} else if load.account.is_empty_code_hash() || load.account.code_hash.is_zero() {
    KECCAK_EMPTY
} else {
    load.account.code_hash
};
```

The third row is always `KECCAK_EMPTY` — the *Standard*, post-iris300 answer.
Correct from iris300 onward; wrong before it.

### The window

EXTCODEHASH exists from **papyrus200 (#2,392,700)** (RSKIP140) and the
behaviour is only correct from **iris300 (#3,614,800)**. So the divergence
spans **#2,392,700 – #3,614,799, about 1.22M blocks**, and that window is
permanently closed.

### Why it is not being fixed as a defect

The whole-chain replay executes every mainnet block from genesis and checks
`gas_used`, `paid_fees` and the receipts root. A different EXTCODEHASH result
feeds a contract's computation and would move a state root, so an exercised
occurrence would have failed the replay. It did not — which places this in the
same family as the `DUPN`/`SWAPN`/`TXINDEX` carve-out: a real divergence in a
closed, proven-unexercised window.

It still matters for **testnet replay**, where the activation ladder differs,
and it should be implemented if that is ever in scope. The fix is a hardfork
gate plus the `ZERO_HASH` branch, and it needs the "is a contract but has no
code value" condition to match rskj's `isContract` exactly — a wrong fix here
is worse than the documented gap, because it would move results inside the
window that currently agree.

### Why the audit rated it INFO

The audit checked whether rustock *had* a gate named for RSKIP169 and found
none. It did not compare the two branches' return values, so the finding reads
as a naming/structure observation rather than a behavioural difference. Reading
rskj rather than the RSKIP is what surfaces it.

---

## FRCR-281 — RSKIP552 gate defined but unused

**Verdict: premise does not hold.** `RskHardforkConfig::has_rskip552` exists
(`hardfork.rs:579`) and is covered by the activation-table tests. A gate that
is defined and tested but whose call site has not landed yet is not a
divergence; it is the ordinary shape of staged work.

## FRCR-577 — EIP-2028 calldata reduction correctly gated at arrowhead600

**Verdict: observation, no action.** The audit is confirming the
implementation is right. Recorded so the row is not re-examined later.

---

# evm — RSKIPs the audit skipped (#149)

## RSKIP-131 — CREATE2 after SUICIDE in the same block

**Verdict: divergence from rskj — fixed.** A live rule on both networks.

From RSKIP125 (wasabi100: mainnet #1,591,000, testnet from genesis), rskj's
`Program.createContract` refuses a creation at an address that an earlier
transaction of the same block deleted (`deletedAccountsInBlock`, which
`BlockExecutor` collects from each `TransactionExecutor` and, in a block run
in sublists, hands from the parallel sublists to the sequential one). The
check comes after the creator's nonce is bumped and the gas is handed over,
so the creation fails as one at an address in use does: the nonce stays
bumped, the gas given to the creation is spent, and 0 is pushed.

rustock let the creation succeed. The test #149 cites as covering this rule,
`selfdestruct_recreate_via_create2_then_selfdestruct_again`, asserted the
opposite of rskj: *"If the second CREATE2 had failed ... the victim would
survive with balance 7"*, and it checked that it did not. In rskj it fails,
and the victim keeps the 7.

Fixed: `deleted_address_collision` (`crates/execution/src/rsk_handler.rs`)
refuses the creation as rskj does, from RSKIP125; `run_sublist` keeps the
block's deletions, and `execute_block_in_parallel` hands the parallel
sublists' to the sequential one. Tests:
`create2_at_an_address_destroyed_earlier_in_the_block_is_refused`, which
replaces the one above, and
`create2_in_the_sequential_sublist_where_a_parallel_one_destroyed_is_refused`,
on one thread and on two.

Reading how rskj tracks the accounts a block deletes and creates turned up
three more differences, fixed with it: FRCR-279 above; an account destroyed
earlier in the block does not exist for the rest of it until something
touches it (`EXTCODEHASH` answers 0, and a call to it pays the new-account
cost); and a contract created with no code keeps its marker though a later
transaction of the block loads it again.

## RSKIP-191 — DUPN, SWAPN and TXINDEX removed

**Verdict: divergence from rskj, closed window — fixed.**

Until RSKIP191 (iris300: mainnet #3,614,800, testnet #2,060,500) rskj's VM
runs three opcodes of RSK's own, and from it throws invalid-opcode for them
(`VM.java`: `OP_DUPN` 0xa8, `OP_SWAPN` 0xa9, `OP_TXINDEX` 0xaa). #46 carved
this out as a closed window, proven unexercised. rustock had none of the
three before iris300: revm has no opcode at 0xa8 to 0xaa, so a contract that
used one failed and spent all its gas where rskj ran on.

Fixed: `rsk_dupn`, `rsk_swapn` and `rsk_txindex`
(`crates/execution/src/rsk_instructions.rs`), installed before RSKIP191 and
replaced by the invalid opcode from it (`has_rskip191`). They follow rskj's
`doDUPN`, `doSWAPN` and `doTXINDEX` in the details: DUPN pops a depth `d` and
is `DUP(d + 1)`, SWAPN is `SWAP(d + 1)`, both at 3 gas, and both step over the
byte after them (rskj calls `program.step()` twice), which a jump may still
land on; a depth the stack does not reach, or one no Java `int` holds, fails
and spends all the gas; TXINDEX pushes the transaction's index in its block,
at 2 gas. Tests: `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191`,
`dupn_and_swapn_step_over_the_next_byte_and_stop_past_the_end` and
`txindex_pushes_the_transactions_place_in_its_block_before_rskip191`.

FRCR-279's analysis above leans on this carve-out. Both windows are now
matched rather than argued unexercised.
