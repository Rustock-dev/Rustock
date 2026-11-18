# misc — info findings

Seven findings, none actionable. Six are the audit correctly observing that a
spec exists which neither rustock nor mainnet implements — which is the audit's
own baseline biting: measured against the RSKIP corpus, an unimplemented
*Accepted* or *Adopted* RSKIP is a deviation; measured against deployed rskj,
it is the status quo.

| finding | RSKIP | verdict |
|---|---|---|
| FRCR-173 | 4 — partition mechanism | absent from rustock **and from mainnet consensus** |
| FRCR-176 | 45 — event tree / extended LOG | absent from rustock **and from mainnet consensus** |
| FRCR-177 | 51 — COREG | absent from both; the RSKIP's own status is contradictory |
| FRCR-174 | 6 — `gasLimit/68` block-size rule | not enforced; matches mainnet |
| FRCR-175 | 6 — zero-byte data discount | not removed; matches mainnet, superseded by Draft RSKIP-44 |
| FRCR-374 | 24 | Adopted RSKIP with an **empty** Specification section, deferring to a Draft |

The seventh is worth stating on its own.

## FRCR-576 — "skip flawed tx, keep block valid" not adopted; rustock aborts

**Verdict: matches shipped rskj.** RSKIP-10 proposes that a flawed transaction
be skipped and the block stay valid. Neither rskj nor rustock does this: both
treat the block as invalid. The audit notes the agreement itself.

Adopting the RSKIP here would mean accepting blocks rskj rejects — the
accepts-too-much failure mode, and a fork in the chain-selection sense.

---

# misc — RSKIPs the audit skipped (#149)

| RSKIP | verdict |
|---|---|
| 0 — RSKIP purpose and guidelines | **no consensus effect**: the process document, not a specification |
| 59 — child contracts | absent from rustock **and from rskj**, which has no rule for it; matches mainnet |

RSKIP-59 is Accepted in the RSKIPs index, which is where #149's status comes
from. Its own file says *"Rejected in favor of RSKIP125"*: CREATE2 is what
was built instead.
