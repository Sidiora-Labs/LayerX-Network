# MCP tools

`layerx-mcp` exposes 21 tools in `TOOL_CATALOGUE`
(`agent/crates/layerx-mcp/src/server.rs`). Every call is a
`DaemonInvocation` through `layerx-agentd` with gates Policy, Capability,
Budget, RateLimit, and Audit. There is no MCP-only write path.

Read-only deployment omits write tools. Encoded JSON arguments are bounded
(catalogue validation 65_536 bytes; route 1_048_576 bytes).

Sources: `agent/crates/layerx-mcp/src/server.rs`,
`agent/crates/layerx-mcp/src/catalogue.rs`,
`agent/crates/layerx-mcp/README.md`,
`spec/.beta/layerx-agent-interface/spec.kvx` requirement 17.

Operator walkthrough: [Running an agent](RunningAnAgent.md). Write
semantics: [Agent API](AgentApi.md).

---

## Catalogue

Descriptions are the strings `catalogue.rs` returns from `description`.

| Tool | Kind | Scope | Daemon operation | Description |
| --- | --- | --- | --- | --- |
| `balance.get` | read | `read:balance` | ReadBalance | Read verified program-scoped balances from the daemon with their verification level and freshness. |
| `history.list` | read | `read:history` | ReadHistory | Page verified account history from the daemon under an explicit item bound and stable cursor. |
| `receipt.get` | read | `read:receipt` | ProgramReceipt | Read the canonical receipt the daemon holds for one activity. |
| `checkpoint.get` | read | `read:checkpoint` | ReadCheckpoint | Read the finalised checkpoint certificate for one batch sequence through the daemon. |
| `proof.get` | read | `read:proof` | ReadProofBundle | Read the proof bundle the daemon holds for one activity. |
| `availability.get` | read | `read:availability` | AvailabilityFetch | Read verified availability chunks and attributed failures for one decimal batch number. |
| `wallet.accounts` | read | `read:wallet:accounts` | ReadAccount | List the verified accounts the daemon observes for one program. |
| `wallet.balance` | read | `read:wallet:balance` | ReadBalance | Read one verified account balance for one asset through the daemon. |
| `activity.prepare` | write | `write:prepare` | Prepare | Prepare canonical activity bytes and their bound disclosure inside the daemon. |
| `activity.disclose` | write | `write:disclose` | Prepare | Decode the bound disclosure of canonical activity bytes. |
| `activity.sign` | write | `write:sign` | Sign | Sign a daemon preparation at the daemon's external signing boundary; no key material is read. |
| `activity.submit` | write | `write:submit` | Submit | Submit an externally signed preparation through the ordinary daemon path. |
| `activity.track` | write | `write:track` | Track | Resolve the daemon's current state for one submission. |
| `activity.wait` | write | `write:activity:wait` | Wait | Wait, under an explicit bound, for the daemon to resolve one submission. |
| `wallet.send` | write | `write:wallet:send` | Submit | Send one asset amount through the ordinary daemon submission path. |
| `token.create` | write | `write:token:create` | Submit | Create one asset through the ordinary daemon submission path. |
| `token.mint` | write | `write:token:mint` | Submit | Mint one asset amount through the ordinary daemon submission path. |
| `token.transfer` | write | `write:token:transfer` | Submit | Transfer one asset amount through the ordinary daemon submission path. |
| `grant.issue` | write | `write:grant:issue` | Submit | Issue one spending grant through the ordinary daemon submission path. |
| `grant.draw` | write | `write:grant:draw` | Submit | Draw against one spending grant through the ordinary daemon submission path. |
| `faucet.request` | write | `write:faucet:claim` | FaucetClaim | Claim one bounded beta faucet grant for the named DID and signer key through the daemon's faucet operation. |

Read tools are absent from the list when the bound scope does not include
them.

---

## Parameters

Required fields from `catalogue.rs` field schemas. Hex lengths are the
catalogue patterns.

| Tool | Required | Optional |
| --- | --- | --- |
| `balance.get` | `program` (hex32) | `account`, `asset` (hex32) |
| `history.list` | `account` (hex32), `limit` (1–256) | `cursor` (hex32) |
| `receipt.get` | `activity_id` (hex32) | — |
| `checkpoint.get` | `sequence` (decimal u64) | — |
| `proof.get` | `activity_id` (hex32) | — |
| `availability.get` | `batch` (decimal u64) | — |
| `wallet.accounts` | `program` (hex32) | — |
| `wallet.balance` | `program`, `account`, `asset` (hex32) | — |
| `activity.prepare` | `activity_type`, `payload`, `account_sequence`, `not_before_ms`, `expires_at_ms`, `fee_limit`, `idempotency_key` | — |
| `activity.disclose` | `canonical_bytes` | — |
| `activity.sign` | `preparation_ref` | — |
| `activity.submit` | `preparation_ref`, `signature` (hex64), `signer_public_key` (hex32) | — |
| `activity.track` | `submission_ref` | — |
| `activity.wait` | `submission_ref`, `timeout_ms` (1–600000) | — |
| `wallet.send` | `destination`, `asset`, `amount`, `idempotency_key` | — |
| `token.create` | `symbol`, `decimals` (0–18), `supply`, `idempotency_key` | — |
| `token.mint` | `asset`, `destination`, `amount`, `idempotency_key` | — |
| `token.transfer` | `asset`, `destination`, `amount`, `idempotency_key` | — |
| `grant.issue` | `beneficiary`, `asset`, `amount`, `expires_at_ms`, `idempotency_key` | — |
| `grant.draw` | `grant_id`, `amount`, `idempotency_key` | — |
| `faucet.request` | `did`, `public_key` (hex32) | — |

Wallet and token writes reuse the ordinary submit path. `activity.wait`
uses the `Wait` operation and existing receipt tracking stages.

---

## Start here

- [Agent API](AgentApi.md)
- [Agentd](Agentd.md)
- [Running an agent](RunningAnAgent.md)
- [Payments developer path](PaymentsQuickstart.md)
