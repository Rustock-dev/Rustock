# mining-consensus — RSKIPs the audit skipped (#149)

## RSKIP-46 — mining fees in the header

**Verdict: matches rskj.**

The header carries `paidFees` from genesis, with no activation, and rskj's
`BlockExecutor.validate` refuses a block whose `paidFees` is not what running
it paid (*"given paidFees doesn't match"*), at every height. rustock makes the
same check after running the block, at every height
(`ProcessError::PaidFeesMismatch`, `crates/execution/src/processor.rs`), and
keeps the field whole however long it is (`paid_fees_past_256_bits_are_kept`,
`crates/core/src/types/header.rs`).

## RSKIP-157 — cumulative difficulty in JSON-RPC block responses

**Verdict: no consensus effect; matches rskj.** JSON-RPC only.

rskj's `cumulativeDifficulty` is `Block.getCumulativeDifficulty`: the block's
difficulty plus each of its uncles'. rustock answers the same
(`cumulative_difficulty`, `crates/rpc/src/helpers.rs`), beside
`totalDifficulty`, which is the chain's up to the block.
