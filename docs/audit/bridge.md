# bridge — info findings

## FRCR-73 — deprecated `getBtcTxHashesAlreadyProcessed` getter absent

**Verdict: no consensus effect.** A deprecated local-only getter. It answers
RPC questions and cannot appear in a transaction's execution, so its absence
cannot fork a chain — it can only give a caller an error where rskj gives an
answer.

Worth having for API parity, at the bottom of the queue behind anything
transaction-callable. #46's priority order puts local-only getters last for
exactly this reason.

---

# bridge — RSKIPs the audit skipped (#149)

## RSKIP-179 — BTC-RSK timestamp linking

**Verdict: matches rskj.**

rskj's `BlockTimeStampValidationRule.isBitcoinTimestampValid`, from RSKIP179
(iris300: mainnet #3,614,800, testnet #2,060,500): a header's timestamp and
that of the Bitcoin header it was merge-mined in must differ by less than
`Constants.getMaxTimestampsDiffInSecs`, strictly. That is 300 s, or 7,200 s on
testnet and testnet2 from RSKIP297 (hop400). A missing or unreadable Bitcoin
header fails the block by itself, and the whole timestamp rule is off where
`validPeriodLength` (`newBlockMaxSecondsInTheFuture`) is 0, regtest's value.
Uncles are held to it too.

rustock's `BitcoinTimestampRule` (`crates/core/src/validation/merged_mining.rs`)
has each of those cases, in the header rules and in the rules each uncle
passes (`rsk_header_rules`, `uncles.rs`). From orchid both nodes require the
Bitcoin-header field to be exactly 80 bytes (rskj's `BtcHeaderSizeRule`), so
both read the same header. Tests in `crates/core/src/validation/tests.rs`
cover each case (`timestamps_are_close_enough`, after rskj's test of that
name, and four more), and an ignored test in
`crates/execution/tests/mainnet_block_rules.rs` holds the rule to a synced
store's mainnet headers on either side of iris300. It came with #347, after
#149 was filed.

## RSKIP-502 — PowPeg and Union Bridge integration

**Verdict: divergence from rskj, live on mainnet — fixed.**

rskj gates the union bridge's seven Bridge methods on RSKIP502, which
`reference.conf` puts at reed810: testnet from #7,139,600, and never mainnet,
whose `main.conf` sets reed810 to -1. On mainnet a call to any of them names
no method, which rskj's `parseData` refuses, and the call fails.

rustock gated them `Since(Reed810)`, an upgrade-order comparison. Mainnet's
schedule goes from reed800 to vetiver900, which ranks above reed810, so the
seven became methods on mainnet at #8,804,200 and stayed so. A transaction
calling one would have run in rustock and failed in rskj, and the node would
have left the chain. `has_rskip144` documents the same trap and was already
written against it.

Fixed: the seven follow `has_rskip502` (`crates/execution/src/hardfork.rs`),
true only from reed810's own height, where reed810 is scheduled. Tests:
`the_union_bridge_is_never_a_method_on_mainnet` (`bridge/mod.rs`), and
`every_rule_activates_where_rskjs_config_puts_it`, which holds each rule
predicate it lists to rskj's heights on both networks.

Where they are methods, on testnet, the seven match rskj's `BridgeMethods`:

| method | gas | static call |
|---|---:|---|
| `setUnionBridgeContractAddressForTestnet(address)` | 8,000 | no |
| `getUnionBridgeContractAddress()` | 3,000 | yes |
| `getUnionBridgeLockingCap()` | 3,000 | yes |
| `increaseUnionBridgeLockingCap(uint256)` | 8,000 | no |
| `requestUnionBridgeRbtc(uint256)` | 8,000 | no |
| `releaseUnionBridgeRbtc()` | 8,000 | no |
| `setUnionBridgeTransferPermissions(bool,bool)` | 8,000 | no |
