# Agent API

The Agent API is the daemon contract in `agent/schema/agent-api/`
(contract major 1, minor 2). It includes identity, write, read, stream,
errors, approval, and programs. The daemon is non-authoritative: it consumes
protocol receipts and never mints balances or protocol budget objects from
local state. See [Agentd](Agentd.md).

Wire:

```text
contract_major:u16be || contract_minor:u16be || request_id:u64be
|| operation:utf8 || payload:canonical-map
```

Amounts, sequences, budget limits, and timestamps travel as decimal strings.
Settlement domain has one variant: `Paxeer` (1.1+).

Sources: `agent/schema/agent-api/*.kvx`,
`spec/.beta/layerx-agent-interface/spec.kvx` requirements 1 and 6.

---

## Write pipeline

`write.kvx`:

| Operation | Required request fields | Response |
| --- | --- | --- |
| `prepare` | actor, authority, account_sequence, timestamp_bound, idempotency_key, fee_limit, payload, payload_hash | `Prepared`: preparation_ref, unsigned_canonical_bytes, signing_preimage, Disclosure, expiry |
| `sign` | preparation_ref, signature | `Signed` |
| `submit` | preparation_ref, signature | `TrackedSubmission` |
| `track` | submission_ref | state, evidence, verification_level, transitions |
| `wait` | submission_ref, requested_verification_level, deadline | submission, actual_verification_level, deadline_elapsed |

Disclosure is decoded from the unsigned canonical bytes and must include
canonical_digest, activity_type, actor, authority, counterparties, amounts,
asset, fee_limit, expiry, and idempotency_key.

`SubmissionState`: Prepared, Signed, Queued, Submitted, Acknowledged,
Unknown (first-class pending), Executed (requires `receipt_ref`; settlement
domain Paxeer), Failed, Expired.

There is no MCP-only write path. MCP tools call these operations. See
[MCP](Mcp.md).

---

## Capabilities

`identity.kvx`. A session carries tenant, agent DID, authority ref,
permitted activity types, expiry, client, and policy_version.

Capability dimensions (all required; empty set denies):

- activity_types
- counterparties
- assets
- amount_ceilings
- rate_ceilings
- purpose_constraints
- expiry

Operations: `capability.create`, `capability.attenuate`, `capability.list`,
`capability.revoke`.

---

## Budgets

`BudgetEnforcement` is either `ProtocolBudget` (protocol-enforced) or
`DaemonLimit` (daemon-enforced; bypassing the daemon bypasses the limit).

Operations: `budget.create`, `budget.fund`, `budget.list`,
`budget.reconciliation`, `budget.revoke`.

`create_protocol_budget` in the daemon submits a verifier-bound activity and
still returns `ProtocolObjectEffectUnavailable` because the receipt schema
carries no created budget object effect ([Agentd](Agentd.md)).

`faucet.claim` is test-network only.

---

## Approval holds

`approval.kvx` and `spec/.beta/layerx-agent-interface/spec.kvx` requirement 6:
holds are daemon-enforced restrictions. They grant no protocol authority.

`ApprovalRecord`: approval_id, tenant, held_activity
(StructuredActivityDisclosure), canonical_bytes_digest, hold_reason,
created_at, expires_at, state.

States: Held, Granted, Rejected, Expired, Defective. Terminal states are
Granted, Rejected, Expired, Defective.

Operations: `approval.list`, `approval.get`, `approval.approve`,
`approval.reject` (idempotency_key on decisions). Approve releases the exact
preparation bound to the disclosure digest.

---

## Subscriptions

`stream.kvx`. Scope is tenant, agent, capability. Filters (all
restrictions): agents, accounts, activity_types, modules, assets,
counterparties, result_classes. Evaluation order is tenant → capability →
filter.

Operations: create (persist + cursor), list, health, acknowledge, pause,
resume, delete.

Delivery is at-least-once. Dedupe uses `deduplication_id`. Variants: Event,
Gap, Truncated. A verified `receipt_reference` is optional (Paxeer).

---

## Reads and errors

`read.kvx`: `read.balance`, `read.account`, `read.module_state`,
`read.history`, `read.batch`, `read.checkpoint`, `read.proof_bundle`,
`availability.fetch`, `export.offline`, `project` (estimate; not a
VerifiedRead).

Verification levels (`errors.kvx`): Unverified → SequencerSigned →
BatchIncluded → StateProven → CheckpointFinalised → SettlementAnchored.

Error classes: TransportFailure, Deadline, ProtocolIncompatibility,
UnavailableCapability, CoreRejection, VerificationFailure, PolicyRefusal,
CapabilityRefusal, BudgetRefusal, RateLimit, IdempotencyConflict,
InternalFault.

Mutations use `IdempotentMutation`: the same body returns the same result;
a different body is `IdempotencyConflict`.

Programs HTTP family (`programs.kvx`): `/v1/programs/deploy`, `/upgrade`,
`/wind-down`, `/simulate`, `/call`, plus registry and receipt reads. See
[Programs](Programs.md).

---

## Start here

- [Agentd](Agentd.md)
- [MCP](Mcp.md)
- [LNI](LNI.md)
- [Running an agent](RunningAnAgent.md)
