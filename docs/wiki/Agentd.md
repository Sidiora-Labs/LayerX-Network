# Agentd

`layerx-agentd` is the non-authoritative LayerX Network agent daemon
(`agent/crates/layerx-agentd/src/lib.rs:1`). It consumes protocol
receipts and signed batch evidence. It never mints balances. It never
mints a protocol budget object from local state.

The crate is a library plus a binary. The binary
(`agent/crates/layerx-agentd/src/main.rs:782-787`) binds a loopback
program-balance listener and a human Unix owner. Budget creation,
approval holds, capability ceilings, and receipt verification are
library paths. Evidence tests construct `EvidenceAuthority` from a
protected `layerx-sequencer-authority-v1` file, then a real sequencer
handshake (`agent/crates/layerx-agentd/tests/support/mod.rs:238-292, 295-309`;
`agent/crates/layerx-agentd/tests/support/real_authority.rs:1293-1298`).

This page covers those library paths. It does not cover MCP routing or
interop adapters.

---

## What it never does

Local figures are a cache, not authority
(`agent/crates/layerx-agentd/src/budget/reconcile.rs:23-29`). A
daemon-only limit is labelled `daemon-enforced` and states that
bypassing `layerx-agentd` bypasses the limit
(`agent/crates/layerx-agentd/src/budget/create.rs:7-8, 84-106`;
`agent/crates/layerx-agentd/tests/budget_create.rs:216-222`).

`create_protocol_budget` submits only a verifier-bound canonical
activity, verifies the core receipt, then still returns
`ProtocolObjectEffectUnavailable` because the receipt schema carries
no created budget object effect. The store argument is unused. No
`ObjectKind::Budget` record is written
(`agent/crates/layerx-agentd/src/budget/create.rs:122-162`;
`agent/crates/layerx-agentd/tests/budget_create.rs:58-83, 87-108`).

`ProtocolBudget` exists as a return type
(`agent/crates/layerx-agentd/src/budget/create.rs:53-82`).
`create_protocol_budget` never returns `Ok`. Those two facts stand
together.

---

## Protocol evidence

Trusted policy is owned by the daemon and is never taken from proof
ingress (`agent/crates/layerx-agentd/src/protocol_evidence.rs:1, 94-100,
502-505`). `Gate::new` loads it from
`StartupConfig.sequencer_authority_source` before any write gate
opens (`agent/crates/layerx-agentd/src/boot.rs:76-93`;
`agent/crates/layerx-agentd/src/protocol_evidence.rs:142-226`). The
file's first line must be `layerx-sequencer-authority-v1`
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:22, 159-161`).

`EvidenceAuthority` is the write-ready wrapper around that verifier
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:429-466`).
`handshake_gate` then requires the node's authorised sequencer key
to match an active configured interval
(`agent/crates/layerx-agentd/src/boot.rs:180-186`;
`agent/crates/layerx-agentd/src/protocol_evidence.rs:228-237`). A
caller cannot substitute a policy after handshake
(`agent/crates/layerx-agentd/tests/protocol_evidence.rs:176-192`).

### Receipt verifier

`EvidenceAuthority::verify_receipt` is the receipt verifier
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:444-454,
311-375`). Raw ingress is `canonical_receipt`, Merkle `proof`,
`canonical_header`, and `header_signature` only
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:502-528`).

Fields that must bind:

| Check | Binding |
| --- | --- |
| Policy | Header `protocol_version`, `network_id`, `sequencer_id`, `epoch`, batch number inside the active (non-revoked) range (`agent/crates/layerx-agentd/src/protocol_evidence.rs:239-302, 315-317`) |
| Inclusion | `layerx_proof::inclusion::verify_receipt` over receipt bytes, proof, header, header signature, and configured `SequencerAuthorization` (`agent/crates/layerx-agentd/src/protocol_evidence.rs:318-325`) |
| Receipt shape | Decode; require a protocol receipt (`agent/crates/layerx-agentd/src/protocol_evidence.rs:326-336`) |
| Protocol version | Receipt protocol version equals the selected header (`agent/crates/layerx-agentd/src/protocol_evidence.rs:337-339`) |
| Sequence range | `global_sequence` inside the header's first/last sequence (`agent/crates/layerx-agentd/src/protocol_evidence.rs:340-344`) |
| Batch identity | `receipt_execution_batch_id(protocol, header)` equals `protocol.batch_id()` (`agent/crates/layerx-agentd/src/protocol_evidence.rs:345-349`; `agent/crates/layerx-agentd/tests/protocol_evidence.rs:342-347`) |
| Outcome | `layerx_proof::receipt::verify_outcome` against `AuthorizedBatch` of execution id, asset, previous/resulting roots, and the configured public key (`agent/crates/layerx-agentd/src/protocol_evidence.rs:350-358`) |

Durable receipt storage runs `verify_outcome` before any index write
(`agent/crates/layerx-agentd/src/receipt.rs:74-123`). Cache values
are constructed only from already-verified evidence
(`agent/crates/layerx-agentd/src/cache.rs:1, 31-36`). Insert refuses
an empty key or a zero `evidence_id`
(`agent/crates/layerx-agentd/src/cache.rs:152-165`).

Failed verification issues no `VerifiedReceiptEvidence`, writes no
cache entry, and writes no budget record
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:305-310`;
`agent/crates/layerx-agentd/src/receipt.rs:48-53, 89-90`;
`agent/crates/layerx-agentd/tests/budget_create.rs:163-188`).

State ingress is the same policy plus
`layerx_proof::inclusion::verify_state`
(`agent/crates/layerx-agentd/src/protocol_evidence.rs:377-405`).

### Evidence refusals

| Error | Meaning |
| --- | --- |
| `VerifierPolicyError::AuthoritySourceUnavailable` / `Unprotected` / `Malformed` | Configured authority file cannot be used (`agent/crates/layerx-agentd/src/protocol_evidence.rs:409-427, 142-156`) |
| `EmptyPolicy` | Zero protocol version, zero network id, or no sequencers (`agent/crates/layerx-agentd/src/protocol_evidence.rs:114-116`) |
| `AmbiguousAuthorization` | Overlapping active sequencer ranges (`agent/crates/layerx-agentd/src/protocol_evidence.rs:107-108, 130-131`) |
| `ProtocolVersion` / `Network` | Header disagrees with trusted policy (`agent/crates/layerx-agentd/src/protocol_evidence.rs:245-250`; `agent/crates/layerx-agentd/tests/protocol_evidence.rs:224-271`) |
| `UnknownSequencer` / `Epoch` / `BatchRange` / `Revoked` | Identity, epoch, range, or inclusive revocation miss (`agent/crates/layerx-agentd/src/protocol_evidence.rs:38, 251-289`; `agent/crates/layerx-agentd/tests/protocol_evidence.rs:196-338`) |
| `HandshakeKey` | Handshake key is not active at the sealed batch (`agent/crates/layerx-agentd/src/boot.rs:180-186`) |
| `ReceiptEvidenceError::Inclusion` / `Receipt` | Proof or `verify_outcome` refusal (`agent/crates/layerx-agentd/src/protocol_evidence.rs:551-560`) |
| `ReceiptEvidenceError::ProtocolVersion` / `SequenceRange` / `BatchIdentity` | Receipt does not bind the selected header (`agent/crates/layerx-agentd/src/protocol_evidence.rs:337-349`) |
| `ReceiptReplayError::DuplicateReceipt` / `DuplicateActivity` | Same receipt digest or activity id admitted twice (`agent/crates/layerx-agentd/src/protocol_evidence.rs:473-500`) |

---

## Budget lifecycle

### Creation from a verified receipt

`BudgetRequest` carries tenant, `request_id`, `BudgetKind`
(`ProtocolBudget` or `CapabilityGrant`), asset, ceiling,
`expiry_sequence`, canonical activity bytes, and an optional
`VerifiedSubmission` (`agent/crates/layerx-agentd/src/budget/create.rs:11-28`).

`create_protocol_budget` refuses a zero ceiling or expiry, an empty
activity, a missing submission, or a submission whose exact bytes or
idempotency key disagree with the request. Those refusals never reach
`BudgetPipeline::submit_budget`
(`agent/crates/layerx-agentd/src/budget/create.rs:137-151`;
`agent/crates/layerx-agentd/tests/budget_create.rs:111-159`). After
submit it verifies the raw receipt and requires
`activity_id` equality and `result_code == 0`, then fail-closes with
`ProtocolObjectEffectUnavailable`
(`agent/crates/layerx-agentd/src/budget/create.rs:152-162`).

`LocalLimit::new` is the honest daemon-only constructor. It does not
submit an activity (`agent/crates/layerx-agentd/src/budget/create.rs:95-106`).

### Reserve

`BudgetLimiter` holds per-`LimitId` ceilings across five scopes:
tenant, agent, session, capability, counterparty
(`agent/crates/layerx-agentd/src/budget/reserve.rs:11-20, 38-82`).
`reserve` checks every applicable limit under one lock, then inserts
the same reservation id into each (`agent/crates/layerx-agentd/src/budget/mod.rs:46-58`;
`agent/crates/layerx-agentd/src/budget/reserve.rs:178-244`).
Projected `consumed + held + requested` may not pass the ceiling
(`agent/crates/layerx-agentd/src/budget/reserve.rs:196-212`).
A refused request leaves no held reservation
(`agent/crates/layerx-agentd/tests/budget_reserve.rs:60-86`).

`release` with `ReleaseKind::Unknown` is a no-op (`Ok(false)`).
`Executed` adds the held amount to `consumed`. `Failed` drops the
hold. `Expired` drops it only when `current_sequence >= expiry`
(`agent/crates/layerx-agentd/src/budget/reserve.rs:153-158, 290-319`;
`agent/crates/layerx-agentd/tests/budget_reserve.rs:89-101`).

This limiter is daemon-local. It is not a protocol budget object.

### Reconcile

`reconcile` verifies every spend receipt against
`expected_activity_id`, admits them through a replay guard, verifies
state inclusion, then returns `ProtocolStateSchemaUnavailable`. Core
defines no canonical budget record/key schema. Local accounting is
not rewritten (`agent/crates/layerx-agentd/src/budget/mod.rs:29-36`;
`agent/crates/layerx-agentd/src/budget/reconcile.rs:7-14, 102-125`;
`agent/crates/layerx-agentd/tests/budget_reconcile.rs:17-39, 114-148`).

`ReconciliationState` has no public constructor
(`agent/crates/layerx-agentd/src/budget/reconcile.rs:31-34`).
`reconcile` never returns `Ok`.

### Divergence

`divergence_alert` would take a `ReconciliationState` with a
`Some(divergence)`, enforce `max(local, protocol)` consumed and
`min(protocol remaining, local remaining)`, and set
`ready_for_writes: false` (`agent/crates/layerx-agentd/src/budget/divergence.rs:32-56`).
Because `reconcile` never issues that state, the live path cannot
open an alert. Untyped included leaves are not authority
(`agent/crates/layerx-agentd/tests/budget_divergence.rs:17-36`).

### Unknown-budget refusal

`hold_unknown` persists an unresolved reservation under
`ObjectKind::Budget` (`agent/crates/layerx-agentd/src/budget/hold.rs:80-87, 147-151`).
Process loss keeps the hold
(`agent/crates/layerx-agentd/tests/budget_unknown.rs:18-56`).

`rebuild` verifies receipts (result code `0` sums `amount` into
`receipt_consumed`), verifies state inclusion, counts unresolved
holds, then sets `protocol_consumed: None` and `reconciled: false`
(`agent/crates/layerx-agentd/src/budget/hold.rs:89-137`).
`require_write_ready` returns `ProtocolStateSchemaUnavailable`
(`agent/crates/layerx-agentd/src/budget/hold.rs:49-57`;
`agent/crates/layerx-agentd/tests/budget_unknown.rs:60-85`).

### Budget states

| Record | What it holds | Authority |
| --- | --- | --- |
| `LocalAccounting` | `consumed`, `window_start_sequence`, `last_receipt` (`agent/crates/layerx-agentd/src/budget/reconcile.rs:23-29`) | Cache only |
| `LimitState` | Configured ceiling, `consumed`, map of held `(amount, expiry)` (`agent/crates/layerx-agentd/src/budget/reserve.rs:32-35`) | Daemon limiter |
| `DurableBudgetReservation` | Canonical digest over reservation id, limit, scope, amount, ceiling, expiry (`agent/crates/layerx-agentd/src/budget/reserve.rs:126-149`) | Restart record for the limiter |
| `UnknownReservation` | Amount and expiry until a receipt resolves it (`agent/crates/layerx-agentd/src/budget/hold.rs:8-16`) | Held, not spendable |
| `RestartAccounting` | `receipt_consumed`, `held_unresolved`; `protocol_consumed` stays `None` (`agent/crates/layerx-agentd/src/budget/hold.rs:32-39, 131-137`) | Writes refused |
| `ProtocolBudget` | Object id, kind, receipt bytes (`agent/crates/layerx-agentd/src/budget/create.rs:53-61`) | Type exists; creation never returns it |
| `ReconciliationState` | Protocol vs local consumed, remaining, divergence (`agent/crates/layerx-agentd/src/budget/reconcile.rs:31-46`) | Type exists; `reconcile` never returns it |

### Budget refusals

| Error | When |
| --- | --- |
| `BudgetCreationError::InvalidLimit` | Zero ceiling or expiry (`agent/crates/layerx-agentd/src/budget/create.rs:137-139`) |
| `EmptyActivity` | Empty canonical bytes (`agent/crates/layerx-agentd/src/budget/create.rs:140-142`) |
| `ActivityBindingUnavailable` / `ActivityBindingMismatch` | Missing or substituted `VerifiedSubmission` (`agent/crates/layerx-agentd/src/budget/create.rs:143-151`) |
| `Submission` | Pipeline did not return a receipt (`agent/crates/layerx-agentd/src/budget/create.rs:45-50, 152`) |
| `UnverifiedReceipt` | `verify_receipt` refused (`agent/crates/layerx-agentd/src/budget/create.rs:153-155`) |
| `ReceiptActivityMismatch` | Receipt activity id ≠ submission activity id (`agent/crates/layerx-agentd/src/budget/create.rs:156-158`) |
| `CoreRejected` | `result_code != 0` (`agent/crates/layerx-agentd/src/budget/create.rs:159-161`) |
| `ProtocolObjectEffectUnavailable` | Verified success still has no object effect (`agent/crates/layerx-agentd/src/budget/create.rs:126-130, 162`) |
| `LimitRefusal::InvalidRequest` | Zero amount, expiry ≤ current sequence, empty limits, or duplicate reservation id (`agent/crates/layerx-agentd/src/budget/reserve.rs:181-195`) |
| `UnknownLimit` | Unconfigured `LimitId` (`agent/crates/layerx-agentd/src/budget/reserve.rs:170, 192`) |
| `Exceeded` | Names limit, ceiling, consumed, held, requested (`agent/crates/layerx-agentd/src/budget/reserve.rs:161-169, 203-211`) |
| `ReconcileError::UnverifiedReceipt` / `UnverifiedProtocolState` | Evidence failed (`agent/crates/layerx-agentd/src/budget/reconcile.rs:92-100, 112-123`) |
| `DuplicateReceipt` / `DuplicateActivity` / `ReceiptActivityMismatch` | Replay or activity bind failed before schema refusal (`agent/crates/layerx-agentd/src/budget/reconcile.rs:111-118`; `agent/crates/layerx-agentd/tests/budget_reconcile.rs:42-87`) |
| `ProtocolStateSchemaUnavailable` | Inclusion succeeded; no canonical budget schema (`agent/crates/layerx-agentd/src/budget/reconcile.rs:124`) |
| `RestartError::ProtocolStateSchemaUnavailable` | Write admission after rebuild (`agent/crates/layerx-agentd/src/budget/hold.rs:49-51`) |

---

## Approval model

Approvals are a daemon-enforced restriction. They confer no protocol
authority. Bypassing `layerx-agentd` bypasses the restriction
(`agent/crates/layerx-agentd/src/approval/mod.rs:24-30, 51-52, 609-616`).
`ApprovalEnforcement` has one variant: `DaemonOnly`.

### Who approves

`ApproverId` is a non-empty string for the human or external
approver (`agent/crates/layerx-agentd/src/policy/approval.rs:42-63`).
`DecisionRequest` carries tenant, approval id, `DecisionKey`,
approver, and current sequence
(`agent/crates/layerx-agentd/src/approval/mod.rs:288-295`).

### What an approval binds

`hold` stores only prepared bytes and their decoded disclosure, never
the caller request. The hold window must close before preparation
expiry. The SHA-256 of unsigned canonical bytes must equal
`disclosure.canonical_digest`. The context agent must equal the
disclosed actor (`agent/crates/layerx-agentd/src/policy/approval.rs:439-524`).

`decide` on the registry applies exactly one `Approve` or `Reject` to
the disclosure digest the approver saw
(`agent/crates/layerx-agentd/src/policy/approval.rs:560-605`;
`agent/crates/layerx-agentd/tests/approval.rs:99-132`). A changed
digest is `DisclosureChanged`. Expiry never approves
(`agent/crates/layerx-agentd/tests/approval.rs:76-96`). Concurrent
decisions have one winner (`agent/crates/layerx-agentd/tests/approval.rs:136-165`).

`ApprovalService::approve` releases that exact held `Prepared` into
`ApprovalSubmissionQueue`. Submit later must present the same
preparation bytes and `submission_ref`
(`agent/crates/layerx-agentd/src/approval/mod.rs:379-486, 169-198`).
If the live preparation no longer equals the hold, the outcome is
`Defective` (`agent/crates/layerx-agentd/src/approval/mod.rs:635-642`).
Reject releases the budget reservation as `Failed`
(`agent/crates/layerx-agentd/src/approval/mod.rs:488-562`).

`hold_reserved` persists the hold with the exact already-acquired
`BudgetReservation` whose id equals the approval request id
(`agent/crates/layerx-agentd/src/policy/approval.rs:463-505`).

### How events are recorded

`ApprovalEvents::emit` appends a `PolicyDecision` audit entry, then
atomically ingests a local-only stream event
(`agent/crates/layerx-agentd/src/approval/events.rs:60-128`). Canonical
bytes are `LXAE` plus kind, sequence, approval id, digest, and
optional principal (`agent/crates/layerx-agentd/src/approval/events.rs:151-168`).
Stream `verification_level` is `UNVERIFIED`
(`agent/crates/layerx-agentd/src/approval/events.rs:97, 106`). That
disagrees with protocol receipt metadata, which stores a
proof-derived `VerificationLevel`
(`agent/crates/layerx-agentd/src/receipt.rs:30-38, 95-100`). Both
are as written: approval lifecycle events are local restrictions,
not protocol receipts.

| Kind | Audit decision | Stream result code |
| --- | --- | --- |
| `Created` | `Requested` | `0` |
| `Granted` | `Allowed` | `0` |
| `Rejected` | `Refused` | `-1` |
| `Expired` | `Failed` | `-2` |

(`agent/crates/layerx-agentd/src/approval/events.rs:23-28, 185-199`)

`DecisionKey` is required for approve/reject idempotency (non-empty,
≤255 bytes, no NUL)
(`agent/crates/layerx-agentd/src/approval/expiry.rs:15-36`). A repeat
of the winning key returns the original outcome; a different key
returns `Conflict` (`agent/crates/layerx-agentd/src/approval/expiry.rs:141-171`).

### Approval states

| State | Meaning |
| --- | --- |
| `AwaitingApproval` | Hold is open (`agent/crates/layerx-agentd/src/policy/approval.rs:75-81, 538`) |
| `Approved` | Approver released the exact preparation |
| `Rejected` | Approver rejected; reservation released |
| `Expired` | `current_sequence >= expires_at_sequence`; no later approve (`agent/crates/layerx-agentd/src/policy/approval.rs:581-592, 691-720`) |
| `Defective` | Held disclosure digest no longer matches canonical bytes (`agent/crates/layerx-agentd/src/policy/approval.rs:664-674`) |

List/get are tenant-scoped. Cross-tenant identifiers return
`NotFound`, not existence (`agent/crates/layerx-agentd/src/approval/mod.rs:357-361`).

---

## Capability and attenuation

A capability is tenant-owned and has no implicit open dimension
(`agent/crates/layerx-agentd/src/capability/mod.rs:47-94`). Missing
any of activity types, counterparties, assets, amount ceiling, rate
ceiling, purposes, or expiry is `MissingDimension`.

`evaluate` checks a `PreparedIntent` in fixed order: expiry,
activity type, counterparty, asset, amount, rate, purpose
(`agent/crates/layerx-agentd/src/capability/mod.rs:177-203`;
`agent/crates/layerx-agentd/tests/capability.rs:47-76`). The first
failure is the named `Refuse` dimension.

### Narrowing

`assert_narrowing` proves the capability is no wider than a
core-resolved `ProtocolScope` on activity types, counterparties,
assets, amount, and expiry
(`agent/crates/layerx-agentd/src/capability/narrowing.rs:9-18, 75-107`).
`ProtocolScope` has no rate or purpose fields. Those dimensions are
classified `DaemonOnly` unless listed in `enforceable_dimensions`
(`agent/crates/layerx-agentd/src/capability/narrowing.rs:109-131`;
`agent/crates/layerx-agentd/tests/narrowing.rs:58-71`). The binding
never becomes the submission authority; `submission_authority`
returns the protocol authority
(`agent/crates/layerx-agentd/src/capability/narrowing.rs:62-66`).
A later wider protocol scope disables the binding
(`agent/crates/layerx-agentd/src/capability/narrowing.rs:42-59`).

### Attenuation

`attenuate` derives a child that is a subset on every dimension.
Amount and expiry may shrink, not grow. Rate `maximum_uses` may
shrink; `window_sequences` may not shorten below the parent
(`agent/crates/layerx-agentd/src/capability/attenuate.rs:169-194, 229-255`).
`revoke_subtree` marks the root and descendants revoked and cancels
only unsubmitted work
(`agent/crates/layerx-agentd/src/capability/attenuate.rs:196-227`).

Enforcement reports refuse wording that calls a daemon-only control
a protocol guarantee
(`agent/crates/layerx-agentd/src/capability/report.rs:9-10, 100-111`).

### Ceilings

`Ceiling` is a capability amount ceiling. `new` leaves
`protocol_reconciliation: None`. `consume` then returns
`Unreconciled` (`agent/crates/layerx-agentd/src/capability/consume.rs:38-55, 265-285`;
`agent/crates/layerx-agentd/tests/ceiling.rs:7-28`).
`rebuild` from verified receipts may restore `consumed` but still
leaves the ceiling unreconciled, so new reservations stay refused
(`agent/crates/layerx-agentd/src/capability/consume.rs:57-108`;
`agent/crates/layerx-agentd/tests/ceiling.rs:65-91`).
`apply_receipt` consumes only a verified terminal receipt whose
activity id and amount match the held reservation; a failed
`result_code` consumes nothing
(`agent/crates/layerx-agentd/src/capability/consume.rs:110-153`).
Unknown outcomes stay held across expiry
(`agent/crates/layerx-agentd/src/capability/consume.rs:175-202`).

That matches budget rebuild: receipt sums are not protocol
reconciliation (`agent/crates/layerx-agentd/src/budget/hold.rs:131-137`).

---

## Local limit versus protocol-equivalent

The code draws the line in three places.

1. **Budget objects.** `LocalLimit.enforcement` is
   `daemon-enforced` with an explicit bypass statement. Tests forbid
   the word `equivalent`
   (`agent/crates/layerx-agentd/src/budget/create.rs:7-8, 95-106`;
   `agent/crates/layerx-agentd/tests/budget_create.rs:216-222`).
   A protocol budget would be a core object id issued from a verified
   receipt; that path fail-closes
   (`agent/crates/layerx-agentd/src/budget/create.rs:126-130, 162`).

2. **Capability dimensions.** `Enforcement::Protocol` cites the
   protocol authority object. `Enforcement::DaemonOnly` uses the same
   bypass sentence as local limits
   (`agent/crates/layerx-agentd/src/capability/narrowing.rs:21-25`;
   `agent/crates/layerx-agentd/src/capability/report.rs:9-10, 64-69`).
   Rate and purpose have no `ProtocolScope` fields, so they cannot be
   protocol-narrowed by `validate`
   (`agent/crates/layerx-agentd/src/capability/narrowing.rs:89-107`).

3. **Approvals.** `APPROVAL_ENFORCEMENT_NOTICE` states the hold
   confers no protocol authority
   (`agent/crates/layerx-agentd/src/approval/mod.rs:24-25`).

`BudgetLimiter` (`LimitId` as `[u8; 16]`, identity scopes) and
`limits::RateLimiter` (`LimitId` as a string, including
`OperationClass`) are different types with the same names
(`agent/crates/layerx-agentd/src/budget/reserve.rs:11-20`;
`agent/crates/layerx-agentd/src/limits/rate.rs:8-55`). They are not
interchangeable.

---

## Admission, quotas, rate, deadlines

These are layered daemon controls, not protocol budgets.

Outbound core work uses `BoundaryAdmission` with seven strict
priority lanes. Submission and receipt resolution outrank bulk reads.
Admission never waits; it returns typed `Backpressure`
(`agent/crates/layerx-agentd/src/limits/admission.rs:3-13, 109-123, 168-174`).

`Quota` prices five durable resources per tenant and sheds only
non-critical work of a pathological client. Submission and receipt
resolution still admit (`agent/crates/layerx-agentd/src/limits/quota.rs:16-25, 264-294`).

`RateLimiter` uses authenticated `logical_time_ms`, never an instance
clock. Every instance of a tenant must share one linearizable
`CounterLedger` (`agent/crates/layerx-agentd/src/limits/rate.rs:5-6, 253-261`).

Every request has a finite `RequestDeadline`. A write that may have
reached core transfers to `UnknownResolving` instead of cancelling
(`agent/crates/layerx-agentd/src/limits/deadline.rs:5-18, 437-458`).

---

## Degraded mode and finality

`Controller` starts `Unreachable` with `UNVERIFIED`
(`agent/crates/layerx-agentd/src/degraded.rs:115-123`). Modes:

| Mode | How entered | Preparation / submit ack / live stream | Unknown resolution |
| --- | --- | --- | --- |
| `Healthy` | `Observation::Ready` without head or level regression | served | served |
| `Behind` | Ready observation with a lower head sequence or lower maximum verification | `LiveCoreRequired` | served |
| `Unreachable` | `Observation::Unreachable` | `LiveCoreRequired` / `StreamUnavailable` | `ResolutionUnavailable` |
| `Halted` / `Emergency` / `DataUnavailable` | matching observation | refused writes and streams | served |

(`agent/crates/layerx-agentd/src/degraded.rs:13-20, 134-139, 228-288, 291-331`)

`serve_cached` returns already-verified bytes with an explicit
staleness flag and a reported level capped by
`maximum_verification`. It refuses a read before any verified
reference (`agent/crates/layerx-agentd/src/degraded.rs:179-226`).
Cache revalidation never serves above the held proof level
(`agent/crates/layerx-agentd/src/cache.rs:219-276`;
`agent/crates/layerx-agentd/tests/cache.rs:74-109`).

Finality records sit beside receipt bytes, never inside them
(`agent/crates/layerx-agentd/src/finality.rs:32, 68`). `augment`
verifies activity and receipt inclusion and raises metadata to
`BATCH_INCLUDED` without changing canonical receipt bytes
(`agent/crates/layerx-agentd/src/finality.rs:78-161`). Checkpoint
finality is unavailable on that raw path.
`augment_verified` may raise to `CHECKPOINT_FINALISED` when a
node-authority `VerifiedCheckpoint` covers the same header. The
comment at the settlement field states registration bytes are
retained and never relabelled as a Paxeer settlement anchor without
a separate live-chain verifier
(`agent/crates/layerx-agentd/src/finality.rs:164-256, 231-233`).
`wait_for_level` polls an independent `VerificationProgress` source
until the requested level or an explicit deadline
(`agent/crates/layerx-agentd/src/finality.rs:282-323`).

---

## Idempotency keys

Caller idempotency is durable per tenant
(`agent/crates/layerx-agentd/src/idempotency.rs:1, 88-93`). The key
must be non-zero; request bytes must be non-empty and ≤ `1_048_576`
(`agent/crates/layerx-agentd/src/idempotency.rs:11-12, 161-163`).
Digest domain is `LXP/agent/request/v1\0`
(`agent/crates/layerx-agentd/src/idempotency.rs:264-268`). A repeat
with the same digest returns `RepeatedOriginal`. A different digest
is `Conflict` (`agent/crates/layerx-agentd/src/idempotency.rs:169-178`).
Daemon retention is never shorter than the protocol window
(`agent/crates/layerx-agentd/src/idempotency.rs:21-37`).

Receipt storage indexes the same 32-byte key
(`agent/crates/layerx-agentd/src/receipt.rs:14-18, 82-86`). Approval
`DecisionKey` is a separate bounded string for approve/reject
(`agent/crates/layerx-agentd/src/approval/expiry.rs:15-36`).

---

## Config keys

Startup configuration is one UTF-8 `key=value` file. Exact
`LAYERX_*` environment values override the named file. No
security-relevant value has a default
(`agent/crates/layerx-agentd/src/config.rs:1-6, 64-65`). Unknown
`LAYERX_*` names are refused (`agent/crates/layerx-agentd/src/config.rs:171-173`).

| File key | Environment key | Role |
| --- | --- | --- |
| `network_id` | `LAYERX_NETWORK_ID` | Non-zero network id (`agent/crates/layerx-agentd/src/config.rs:29-62, 295`) |
| `node_endpoint` | `LAYERX_NODE_ENDPOINT` | Absolute normalised node path |
| `expected_protocol_version` | `LAYERX_EXPECTED_PROTOCOL_VERSION` | Occupancy protocol; unsupported versions refused (`agent/crates/layerx-agentd/src/config.rs:296-302`) |
| `tenants` | `LAYERX_TENANTS` | Non-empty unique tenant ids |
| `policy_sources` | `LAYERX_POLICY_SOURCES` | Exactly one absolute path per tenant |
| `signer_configurations` | `LAYERX_SIGNER_CONFIGURATIONS` | Exactly one absolute path per tenant |
| `verification_defaults` | `LAYERX_VERIFICATION_DEFAULTS` | Per-tenant `sequencer-signed`, `batch-included`, `state-proven`, `checkpoint-finalised`, or `settlement-anchored` (`agent/crates/layerx-agentd/src/config.rs:438-471`) |
| `sequencer_authority_source` | `LAYERX_SEQUENCER_AUTHORITY_SOURCE` | Protected `layerx-sequencer-authority-v1` file |

| `RejectionReason` | Meaning |
| --- | --- |
| `Missing` / `Empty` / `Duplicate` / `Unknown` | Absent, blank, repeated, or unrecognised setting (`agent/crates/layerx-agentd/src/config.rs:80-94`) |
| `InvalidInteger` / `UnsupportedProtocol` | Zero or non-occupancy protocol |
| `InvalidTenant` / `IncompleteTenantMap` | Bad or incomplete per-tenant maps |
| `InvalidPath` | Not absolute and normalised |
| `InvalidVerificationLevel` | Unsafe default |
| `TooLarge` / `InvalidEncoding` / `Unavailable` / `Unprotected` | File bounds, UTF-8, I/O, or mode/owner |

The binary in `main.rs` reads a disjoint `LAYERX_AGENT_*` set for
the program-balance listener and human owner (loopback listen,
distinct bearers, deployment journal, human socket, one
`LimitConfig`). Those names are required environment keys, not
`SECURITY_RELEVANT_SETTINGS` (`agent/crates/layerx-agentd/src/main.rs:61-67, 114-135, 514-549`).

---

## Makefile gates

Agentd suites live under `make agent-test-agentd-<area>` in the
repository `Makefile`. Budget, approval, capability, evidence, cache,
degraded, finality, and idempotency targets include:

- `agent-test-agentd-budget-create` / `budget-reconcile` / `budget-reserve` / `budget-unknown` / `budget-divergence`
- `agent-test-agentd-approval` and `agent-test-approvals`
- `agent-test-agentd-capability` / `narrowing` / `attenuation` / `ceiling` / `enforcement-report`
- `agent-test-agentd-cache` / `receipts` / `finality` / `degraded` / `idempotency` / `handshake-gate` / `admission` / `config`

(`Makefile:2357-2385, 2397-2401, 2426-2436, 2453-2454, 2478-2479, 2522-2529`)

`tests/protocol_evidence.rs` has no dedicated `agent-test-agentd-protocol-evidence` target.
The handshake and budget suites load the same `EvidenceAuthority` path.

Sources: `agent/crates/layerx-agentd/src/{lib,main,config,protocol_evidence,receipt,cache,degraded,finality,idempotency,authority}.rs`,
`agent/crates/layerx-agentd/src/{budget,approval,capability,limits,policy,boot}/`,
`agent/crates/layerx-agentd/tests/{budget_*,approval*,capability,ceiling,degraded,finality,cache,admission,protocol_evidence}.rs`,
`agent/crates/layerx-agentd/tests/support/{mod,real_authority}.rs`.

[Home](Home.md)
