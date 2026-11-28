# Serving an SPV wallet

What a light wallet needs from a rustock node, what it can and cannot check
for itself, and why **running `--spv`** and **serving an SPV wallet** are two
different jobs that happen to share a word.

---

## 1. The distinction, first, because it is the thing most often got wrong

`--spv` is a flag on *this* node. It makes rustock a light client: it holds
headers and **no trie**, and answers state questions by asking a peer over
`rsk/64` and verifying the reply against a header it validated itself.

Serving an SPV wallet is the opposite role. The node is the **source** of
state: it holds the trie, reads answers out of it, and hands them over with
the nodes that prove them.

```
  ┌────────────┐   RPC    ┌──────────────┐   RPC    ┌───────────────┐
  │ SPV wallet │ ───────► │ wallet server│ ───────► │ rustock node  │
  │  (mobile)  │          │  (stateless  │          │  FULL — holds │
  └────────────┘          │   relay)     │          │  the trie     │
                          └──────────────┘          └───────────────┘

  the --spv deployment is a different shape entirely:

                          ┌───────────────┐  rsk/64  ┌───────────────┐
                          │ rustock --spv │ ───────► │ some peer     │
                          │ holds NO trie │ ◄─────── │ holds the trie│
                          └───────────────┘  proofs  └───────────────┘
```

**A wallet server connects to a full node.** Not to an `--spv` node. The two
are both "SPV" in the sense that both verify rather than trust, but one is
asking and the other is answering, and they need opposite things from their
storage.

### Why this matters in practice, and not just as terminology

An `--spv` node cannot serve a wallet server properly. It is missing the data:

| method | full node | `--spv` node |
| --- | --- | --- |
| `eth_getProof` | from its own trie | relayed from a peer, verified |
| `eth_getCode` | from its own trie | relayed from a peer, verified |
| `eth_getBalance`, `eth_getTransactionCount`, `eth_getStorageAt` | yes | **fails** — no state |
| `eth_call`, `eth_estimateGas` | yes | **fails** — no state to execute against |
| `eth_getLogs` | yes | **empty, and no error** — see below |
| `eth_getBlockReceipts` | yes | **null** — holds no receipts |
| `rsk_lastAccountChange` | yes, by local bisection | yes, by bisection over peer proofs |

Note the asymmetry on the first three rows: on an `--spv` node `eth_getProof`
works and `eth_getBalance` does not, which surprises people. The proof path
has a peer to ask; the plain state getters go straight to a trie that is not
there.

The `eth_getLogs` row is the dangerous one. It does not fail — it holds no
receipts, finds nothing, and returns `[]` with no error. A caller cannot tell
that from "there genuinely were no matching logs", so pointing a wallet
server at an `--spv` node produces a silently empty history rather than a
visible outage.

So the `--spv` work in this repository is not wallet-serving infrastructure.
It is a separate, real deployment — a node that follows the chain cheaply —
and it happens to exercise the same proof machinery from the other side.
That shared machinery is why the two get confused.

---

## 2. What a wallet needs from a full node

Grouped by the question being asked, with the trust property of each answer.
"Verifiable" means the wallet can check it against a header it already
trusts, without trusting the node.

### 2.1 Following the chain

| need | method | verifiable |
| --- | --- | --- |
| headers in bulk, with their uncles | `rsk_getRawBlocks(from, to)` | yes — PoW and merged mining, from a checkpoint |
| one header | `rsk_getRawBlockHeaderByNumber` / `ByHash` | yes |
| live head | `eth_subscribe("newHeads")` | yes, once checked as above |

`rsk_getRawBlocks` returns `{header, uncles}` per block, up to 1,000 a call,
oldest first, stopping before the first block it does not hold whole or that
does not follow its predecessor. The uncles matter and are not decoration:
**RSK total difficulty advances by the trunk block's difficulty plus every
uncle's**, and uncle headers travel only in the body, so a client holding
headers alone cannot compute the chain's work.

Budget for the uncles, not just the headers. Measured over blocks
#9,448,072–#9,449,071: the trunk header is 1,128 B mean, the uncles add
1,285 B — **53% of the bytes** — and 658 of 1,000 blocks carry at least
one, the most being seven. So catching up costs **~2,414 B a block: 6.9 MB a
day, 209 MB a month**, in ~87 calls. Over the whole chain that ratio holds:
`RSKIP-cumulative-difficulty-in-header` puts it at 10.5 GB of uncle headers
against 10.1 GB of trunk headers.

An earlier version of this file said 97 MB a month. That figure costed the
trunk headers and left the uncles out, which is the same mistake as not
reading the paragraph above it.

### 2.2 Account state

| need | method | verifiable |
| --- | --- | --- |
| balance, nonce | `eth_getProof` → `accountProof` | **yes** |
| a storage slot | `eth_getProof` → `storageProof[i].proof` | **yes** |
| that an account does **not** exist | `eth_getProof` → divergence path | **yes** |
| that a slot is zero | same — a zero slot is *deleted* from the trie | **yes** |
| `codeHash` | `eth_getProof` | **not by default** — see §3 |
| `storageHash` | `eth_getProof` | **not by default** — see §3 |
| contract code | `eth_getProof` with `includeCode`, or `rsk_getStateNodes` | yes, when asked for |
| raw trie walk, any key | `rsk_getStateNodes(blockHash, [keys])` | yes |

A never-used address is a new wallet's first screen, so the absence case is
the common one rather than the edge one. It is answered with the **divergence
path** — the nodes down to where the search stops — which the wallet verifies
exactly as it verifies an inclusion path. An **empty** proof array is not an
absence proof; it means the node could not walk its trie, and the wallet must
treat it as a failure to answer.

### 2.3 What a block did

| need | method | verifiable |
| --- | --- | --- |
| **every** receipt of a block | `eth_getBlockReceipts(block)` | **yes** — rebuild the receipts trie, compare to `receiptsRoot` |
| one receipt, proved included | `rsk_getTransactionReceiptNodesByHash` | yes, *inclusion only* |
| one receipt, raw | `rsk_getRawTransactionReceiptByHash` | no, on its own |

The distinction in the last two rows is the point. A Merkle branch proves
receipt 3 is in the block and says **nothing about a receipt 7 the responder
chose not to mention**. For a wallet reconstructing what a block did to an
account, the failure that matters is a withheld receipt, and the only sound
check is to take the whole set and rebuild the root. That is what
`eth_getBlockReceipts` is for; it returns the entire set or an error, never a
prefix.

### 2.4 Finding history without scanning

| need | method | verifiable |
| --- | --- | --- |
| the block an account last changed in | `rsk_lastAccountChange` | yes — each probe is a proof |

Native rBTC is the hard case: a value transfer emits no log, sets no bloom
bit, and is invisible to a client watching headers. `rsk_lastAccountChange`
bisects on "does the record here already equal the record at the top", which
is false below the change and true from it onward.

**Off by default** — it costs ten to twenty-four state lookups per call, so it
is behind `--rpc-account-search`. An operator serving wallets must turn it on.

Its answer carries `complete`, and reading the answer without it is the one
mistake that silently drops a payment from a user's history: `complete: false`
means *the change is older than the range searched*, not that there was none.

Measured cost of the bisection, from
`crates/cli/tests/spv_account_tracing.rs`: **24 proofs to recover 4 changes
over 24 blocks.** A consensus-level account-modification witness would make
each hop one proof instead; that ratio is the argument for the RSKIP, and it
is produced by running the code rather than asserted.

### 2.5 Knowing whether the answers are worth anything

| need | method |
| --- | --- |
| head number, hash, timestamp and **age** | `rsk_nodeStatus` |
| the oldest block whose state this node can prove | `rsk_nodeStatus.oldestProvableBlock` |
| the oldest block it holds at all | `rsk_nodeStatus.oldestBlock` |

Both are things a wallet cannot work out and must not assume.

**Freshness.** `eth_syncing` compares this node's height to the highest it
knows to exist — and with no peers, the highest it knows to exist is its own
head. A node whose sync died hours ago therefore answers `false`: fully
synced. Every proof it then returns is *valid*, hashing to a real state root
in a real validated header, and every one is about a block from hours ago.
A balance screen rendered from a three-day-old header looks exactly like a
current one. `eth_blockNumber` does not help either: it reports the last known
height, which climbs to a number and stops, with nothing saying it stopped.

So `rsk_nodeStatus` publishes `headAgeSeconds` and the wallet sets its own
policy. An operator who wants the node to refuse instead can set
`--rpc-max-head-age`, which fails `latest` reads past the bound with `-32001`
and leaves explicit block numbers alone. It is **off by default**: the right
bound depends on this chain's real inter-block variance, which has not been
measured, and a bound set below it would fail healthy nodes during an
ordinary slow stretch.

**The floor.** `oldestProvableBlock` is `null` when unknown, and `null` must
not be read as zero — zero would claim the whole chain is provable. A wallet
has to draw the line between history it can prove and history it cannot
reach, and show it; the alternative is history that stops quietly, which a
user reads as "nothing happened before this". That reading is wrong in
precisely the case where it matters, a wallet restored from a seed whose
older activity predates the node's window.

### 2.6 Spending

| need | method | verifiable |
| --- | --- | --- |
| submit a signed transaction | `eth_sendRawTransaction` | n/a |
| what to pay | `eth_gasPrice` | **no** |
| what it will cost | `eth_estimateGas` | **no** |
| which chain this is | `eth_chainId` | pin it in the client |

---

### 2.7 Which of these rskj also serves, and which are rustock's alone

A wallet built on the list above **cannot run against an rskj node**, and it
is worth knowing exactly where the line falls rather than discovering it
against a node you do not control.

| method | rskj | notes |
| --- | :---: | --- |
| `eth_getBalance`, `eth_getTransactionCount`, `eth_getCode`, `eth_getStorageAt` | ✅ | |
| `eth_getTransactionReceipt`, `eth_getLogs`, `eth_call`, `eth_estimateGas` | ✅ | |
| `eth_sendRawTransaction`, `eth_gasPrice`, `eth_chainId`, `eth_syncing` | ✅ | |
| `eth_subscribe("newHeads")` | ✅ | over WebSocket, off by default on both |
| `rsk_getRawBlockHeaderByHash` / `ByNumber` | ✅ | |
| `rsk_getTransactionReceiptNodesByHash` | ✅ | `RskModuleImpl`; the receipt-proof shape this node reuses |
| `rsk_getRawTransactionReceiptByHash` | ✅ | main chain only, as `receiptStore.getInMainChain` |
| **`eth_getProof`** | ❌ | rskj does not implement it at all |
| **`eth_getBlockReceipts`** | ❌ | geth/reth name, no rskj equivalent |
| **`rsk_getRawBlocks`** | ❌ | rustock's own |
| **`rsk_getStateNodes`** | ❌ | rustock's own |
| **`rsk_getStorageNodes`** | ❌ | rustock's own |
| **`rsk_getBlockNumberByHash`** | ❌ | rustock's own |
| **`rsk_lastAccountChange`** | ❌ | rustock's own; emulates an unactivated RSKIP |
| **`rsk_nodeStatus`** | ❌ | rustock's own |

**Every method a wallet depends on for verification is in the lower half.**
That is not an accident and it is not fixable by being careful: rskj has no
`eth_getProof`, so there is no proof-shaped answer to fall back on. A client
pointed at rskj gets `Method not found` for the thing it exists to do.

So the compatibility question is not "will this degrade on rskj" — it will
not run. Plan for a rustock node, and detect the wrong one early:
`rsk_nodeStatus` answering `Method not found` is a cheap probe at startup,
and a clearer failure than the first `eth_getProof` returning nothing.

#### The compatibility rule the additions follow

Nothing above **changes** an existing rskj method's behaviour. The additions
are of two kinds only:

- **new methods**, which an rskj node answers `Method not found`;
- **new optional arguments and extra response fields** on existing methods,
  where the method without them behaves exactly as rskj's does.

`eth_getProof`'s fourth parameter (§3) is the second kind, and so are
`eth_syncing`'s extra fields and `rsk_storageStatus`'s `collectedBelow`. A
caller that ignores what it does not recognise sees no difference.

The one deliberate exception is `--rpc-max-head-age`, which makes `latest`
reads *fail* past a bound. It is off by default for exactly that reason, and
turning it on is an operator's choice to diverge.

---

## 3. The trust boundary, stated plainly

This is the part worth reading twice, because one field looks proved and is
not.

**`accountProof` proves the account record.** On Ethereum that record contains
`codeHash` and `storageRoot`, so proving the record proves them. **RSK's
record is `(nonce, balance, stateFlags)` and contains neither.**

So a wallet that verifies `accountProof` against its own header has
established the balance and the nonce — and has established *nothing whatever*
about `codeHash` and `storageHash`. The node read those out of its own trie
and is asserting them. The fields are present, EIP-1186 shaped, and
unsupported by the proof sitting next to them.

That is a trust hole in the middle of a proof, and the field's presence makes
it look closed.

The fourth parameter asks for the evidence:

```json
["0xabc…", [], "latest", {"includeCode": true, "includeStorageRoot": true}]
```

- `codeProof` — the walk to `code_key(address)`. Check with `verify_path`;
  the value it reaches *is* the code, and hashing it must give `codeHash`.
- `storageRootProof` — the walk to `storage_prefix_key(address)`. Check with
  **`verify_node_at`**, not `verify_path`: `storageHash` is the hash of that
  *node*, and the node ordinarily carries no value, so asking for its value
  would answer "absent" for every account that has storage.

Both are off unless asked for. A code proof is the one path whose size grows
with the contract rather than with the depth of the trie, since its leaf is
the code, and a caller that only wanted a balance should not pay for it.

Why a proof of the code key is the *only* route here: with no `codeHash` in
the account record, the Ethereum trick of fetching code by hash and comparing
is unavailable. Authenticating code on this chain means proving `code_key`
or nothing. That is what stands between a wallet and a verified `eth_call` —
which is in turn the only way to show an ERC-20 balance that does not require
a per-token storage-layout oracle.

### What a wallet can never verify

Worth naming, so a client does not pretend otherwise:

- **gas price and gas estimates** — predictions, not facts about a block;
- **the transaction pool** — pending state is not committed to by anything;
- **`eth_getLogs` results** — the bloom narrows a search, it does not prove a
  log set is complete; rebuild from `eth_getBlockReceipts` if completeness
  matters;
- **head freshness** — only its *age* is reported, and only as the node's own
  claim about its clock.

---

## 4. Operator checklist

A node intended to sit behind a wallet server:

| | why |
| --- | --- |
| **archival, or a known floor** | a pruning node can prove only above `collected_below`; publish it and the wallet draws the line honestly |
| `--rpc-account-search` | otherwise `rsk_lastAccountChange` is `Method not found` and history recovery is a full scan |
| consider `--rpc-max-head-age` | once measured; until then the wallet polls `rsk_nodeStatus` |
| **not** `--spv` | see §1 — it holds no trie and cannot answer most of §2 |

CORS is already permissive on both the HTTP and WebSocket handlers, so a
browser can call a node directly. There is **no TLS, no authentication and no
rate limiting** in the RPC layer, and method exposure is all-or-nothing apart
from three narrow gates — `--rpc-admin` (six methods), `--rpc-account-search`
(one) and `--dev-rpc` (the `evm_*` namespace) — plus one bound,
`--rpc-logs-max-blocks`. A wallet needs eight or nine methods; everything
else — `debug_trace*`, `eth_getLogs`, `eth_call`, `eth_estimateGas` — is
reachable by anonymous callers. Put a node serving wallets behind something
that terminates TLS and limits rate, or restrict it at the network layer.

---

## 5. Where this lives in the code

| piece | file |
| --- | --- |
| `eth_getProof`, `rsk_getStateNodes`, `rsk_getStorageNodes`, the light path | `crates/rpc/src/proof.rs` |
| proof verification (`verify_path`, `verify_node_at`, `Outcome`) | `crates/trie/src/proof.rs` |
| the divergence walk | `TrieNode::try_nodes_toward`, `crates/trie/src/node.rs` |
| `rsk_lastAccountChange` and its bisection | `crates/rpc/src/witness_search.rs` |
| `rsk_nodeStatus`, `rsk_getRawBlocks` | `crates/rpc/src/rsk.rs` |
| `eth_getBlockReceipts` | `crates/rpc/src/tx.rs` |
| the `rsk/64` proof messages | `crates/networking/src/protocol/rsk.rs`, and `docs/wire-protocol.md` §4b |
| serving them | `crates/sync/src/snap/server.rs` |
| asking for them, as an `--spv` node | `crates/sync/src/proof_client.rs` |
| the whole journey, end to end | `crates/cli/tests/spv_account_tracing.rs` |

`spv_account_tracing.rs` is the one to read first if you want to see the
shape: it builds a regtest chain, stands up a node with an empty trie, and
has a wallet recover an account's payment history through it — against a
peer, over verified proofs, with no state on either side of the RPC.
