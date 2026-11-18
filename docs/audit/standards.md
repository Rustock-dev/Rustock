# standards — RSKIPs the audit skipped (#149)

#149 put these two first to check, on the thought that a registry
pre-deployed at a fixed address would be genesis state, "either right or a
fork with no middle ground". Neither is genesis state, and neither is anything
a node implements.

## RSKIP-148 — ERC1820 pseudo-introspection registry

**Verdict: premise does not hold.**

rskj has no code for it, and its genesis files (`genesis/rsk-mainnet.json`,
`genesis/orchid-testnet.json`) allocate one account each, the Bridge's
21,000,000 RBTC, with no code. The registry at
`0x1820a4B7618BdE71Dce8cdc73aAB6C95905faD24` is on both chains because an
ordinary transaction put it there: ERC-1820's keyless deployment, whose
sender, `0xa990077c3205cbDf861e17Fa532eeB069cE9fF96`, has nonce 1 on both.
rustock runs that transaction as it runs any other, and every block of both
networks has been re-run and matches its header, state root included.

## RSKIP-159 — minimal proxy contract

**Verdict: premise does not hold.**

EIP-1167 is a bytecode pattern for contracts to use. rskj has no code for it
and nothing of it is in genesis: a proxy is ordinary bytecode, which rustock's
EVM runs as it runs any other.
