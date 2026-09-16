# Human journeys

The browser product contract is `human/schema/human-api/` (major 1,
minor 4 in `v1.kvx`). Journey vocabulary lives in `journeys.kvx`.
Identity, passkeys, and sessions live in `identity.kvx`. Movement
quote/commit lives in `movement.kvx`. Executable wire examples are
the golden files under `human/schema/human-api/golden/`.

Normative product behaviour is in `spec/layerx-platform/spec.kvx`.
The published tree does not contain
`spec/layerx-agent-interface/spec.kvx`; the agent-boundary spec that
is present is `spec/.beta/layerx-agent-interface/spec.kvx`.

This page is the `/v1` journey map. Hosted-cluster material assembly
is [Hosted Human](HostedHuman.md). Operator owner bootstrap is
[Human owner registration](HumanOwnerRegistration.md). Custody and
verification levels are [Human custody](HumanCustody.md).

Every response envelope is `{ ok, result?, error?, trace }`
(`v1.kvx`). Mutations that move money require header
`Idempotency-Key`. Live updates use `POST /v1/stream` and
`GET /v1/stream/{cursor}` (`stream.kvx`).

---

## Journey vocabulary

`JourneyKind` variants
(`human/schema/human-api/journeys.kvx`):

`onboarding`, `wallet-binding`, `deposit`, `withdraw`, `exit`,
`move`, `agent-create`, `agent-fund`, `agent-pause`, `agent-retire`

`deposit`, `withdraw`, and `exit` name custody-boundary journeys
only. Every movement inside LayerX is `move`, resolved to `fund`,
`allocate`, `return`, or `transfer`.

`JourneyState`: `getting-ready`, `sending`, `processing`, `done`,
`done-finalised`, `still-checking`, `refused`, `waiting-for-you`

A `Journey` has `journey_id`, `kind`, `state`, `state_copy_key`,
`stages[]`, `evidence[]`, `started_at`, `updated_at`, and optional
`refusal` and `wallet_request`.

Reads: `GET /v1/journeys`, `GET /v1/journeys/{journey_id}`,
`GET /v1/evidence/{evidence_id}`.

`JourneyKind` includes `agent-fund` and `agent-pause`. Pause
returns an `Agent`, not a `Journey`. Funding has no dedicated HTTP
operation; use `move` quote/commit (and the create journey's
first-funding stage).

---

## Sign-in

Sign-in is not a `JourneyKind`. It is the session prerequisite
(`identity.kvx`).

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Begin passkey | `POST /v1/passkeys/assertions` | optional `email` | `assertion_id`, `ceremony`, `expires_at` |
| Finish passkey | `POST /v1/passkeys/assertions/{assertion_id}` | `credential` | `assertion_id`, `passkey_id`, `completed_at`, `expires_at` |
| Open session | `POST /v1/sessions` | `assertion_id`, optional `device` | `session_id`, `device`, `opened_at`, `last_active_at`, `current` |
| Refresh | `POST /v1/sessions/refresh` | empty | `Session` |
| Fee policy | `GET /v1/sessions/fee-policy` | empty | `asset_id`, `currency`, `decimals` |

Related: `GET /v1/sessions`, `DELETE /v1/sessions/{session_id}`,
`POST /v1/sessions/revoke-all`.

---

## Onboarding

Kind: `onboarding`. The account reads as active only when the
protocol-identity stage carries evidence of class `layerx-receipt` at
verification `receipt-verified` or higher
(`identity.kvx` `operation.onboarding.status`).

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Create account | `POST /v1/accounts` | `email`, `display_name` | `account_id`, `onboarding` (`Journey`) |
| Register passkey | `POST /v1/passkeys/registrations` then `POST .../{registration_id}` | begin: `account_id`; finish: `credential` | challenge, then `Passkey` |
| Status | `GET /v1/onboarding` | empty | `Journey` |
| Resume | `POST /v1/onboarding/resume` | empty | `Journey` |

Stage `copy_key` values in the golden status response:
`onboarding.stage.creating-your-account`,
`onboarding.stage.adding-your-passkey`,
`onboarding.stage.setting-up-your-protocol-identity`,
`onboarding.stage.putting-recovery-in-place`.

Service-internal stages (`human/crates/layerx-human-service`) map
those copy keys onto `ApplicationIdentity`, `CustodyKey`,
`DidRegistration`, `InitialFunding`, and `RecoveryRegistration`.

---

## Wallet binding

Kind: `wallet-binding`.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Statement | `POST /v1/wallet-binding/statement` | `address` | `statement`, `address`, `expires_at` |
| Submit | `POST /v1/wallet-binding` | `address`, `statement`, `signature` | `Journey` |
| Status | `GET /v1/wallet-binding` | empty | `state` ∈ `none` / `binding` / `bound` / `rebinding`; optional `address`, `bound_at`, `evidence` |
| Rebind prep | `POST /v1/wallet-binding/rebind/action` | `address` | `binding`, `confirms` |
| Rebind | `POST /v1/wallet-binding/rebind` | `address`, `statement`, `signature`, `step_up` | `Journey` |

Stages: `wallet-binding.stage.checking-your-signature`,
`wallet-binding.stage.recording-the-link`, and on rebind
`wallet-binding.stage.confirming-it-is-you`. First deposit may fold
in `deposit.stage.linking-wallet`.

---

## Deposit

Kind: `deposit`. Custody-boundary only.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Start | `POST /v1/deposits` | `money` `{amount, currency}`; optional `settlement_domain` | `Journey`, often with `wallet_request` |
| Confirm | `POST /v1/deposits/{journey_id}/confirm` | `wallet_transaction`; optional `settlement_domain` | `Journey` |
| Poll | `GET /v1/journeys/{journey_id}` | | `Journey` |

Stages in goldens: `deposit.stage.waiting-for-wallet`,
`deposit.stage.confirming-on-paxeer`, `deposit.stage.crediting`.
The sign copy key is `deposit.sign.custody-transaction`.

---

## Move

Kind: `move`. Quote then commit (`movement.kvx`). Mechanism is
resolver-derived, not user-chosen: `fund`, `allocate`, `return`,
or `transfer`.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Quote | `POST /v1/moves/quote` | `source`, `destination`, `money` | `quote_id`, `description_copy_key`, `mechanism`, `money`, `fee_estimate`, `fee_ceiling`, `arrival_estimate`, `expires_at`; optional `irreversibility_copy_key` |
| Commit | `POST /v1/moves` | `quote_id` | `Journey` |

A refused journey carries `Refusal`: `refused_by`, `copy_key`,
`money_left`, optional `change_path`.

---

## Withdraw

Kind: `withdraw`. Start uses authorization class `withdrawal`.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Start | `POST /v1/withdrawals` | `money`, `destination`; optional `settlement_domain` | `Journey` |
| Claim | `POST /v1/withdrawals/{journey_id}/claim` | `claim_signature`; optional `settlement_domain` | `Journey` |

Stages in the claim golden: `withdraw.stage.processing`,
`withdraw.stage.waiting-for-settlement`,
`withdraw.stage.ready-to-claim`, `withdraw.stage.paying-out`.

---

## Emergency exit

Kind: `exit`.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| Eligibility | `GET /v1/exit/eligibility` | empty | `eligible`, `copy_key`; optional `withdraw_instead_path`, `settlement_domain` |
| Start | `POST /v1/exit` | `confirmation`; optional `settlement_domain` | `Journey` |

The golden confirmation phrase is exactly `get my money out`
(`human/schema/human-api/golden/exit.start.request.json`). Stages:
`exit.stage.getting-ready`, `exit.stage.waiting-for-wallet`,
`exit.stage.confirming-on-paxeer`. Wallet copy key:
`exit.sign.exit-claim`.

---

## Approvals

Not a `JourneyKind`. Held activities use journey state
`waiting-for-you`. `ApprovalState`: `pending`, `approved`,
`rejected`, `expired`, `defective`.

| Step | Operation | Request | Response |
| --- | --- | --- | --- |
| List | `GET /v1/approvals` | | `approvals[]`, `next_cursor` |
| Detail | `GET /v1/approvals/{approval_id}` | | `facts` (`amount`, `counterparty`, `asset`, `fees`, `expires_at`), `budget_remaining_after`, `evidence` |
| Step-up | `POST /v1/step-up` then finish | `confirms`; finish `credential` | `StepUpEvidence` |
| Approve | `POST /v1/approvals/{approval_id}/approve` | `step_up_evidence` | `money_moved`, `moved_copy_key`, `evidence` |
| Reject | `POST /v1/approvals/{approval_id}/reject` | empty | `ApprovalDecision` |

---

## Managed agents

### Create

Kind: `agent-create`. `POST /v1/agents` with `name`, `purpose`,
`monthly_limit`; optional `native_fee_budget`. Returns a `Journey`.
Stages in the create golden: `agent.create.stage.setting-up`,
`agent.create.stage.protection`, `agent.create.stage.first-funding`.

### Controls

| Action | Operation | Request | Response |
| --- | --- | --- | --- |
| List / get | `GET /v1/agents`, `GET /v1/agents/{agent_id}` | | `Agent` / page |
| Pause | `POST /v1/agents/{agent_id}/pause` | empty | `Agent` |
| Resume | `POST /v1/agents/{agent_id}/resume` | empty | `Agent` |
| Limit | `POST /v1/agents/{agent_id}/limit` | `monthly_limit` | `Agent` |
| Reclaim | `POST /v1/agents/{agent_id}/reclaim` | `money` | `Journey` (move / return) |
| Rotate | `POST /v1/agents/{agent_id}/rotate` | empty | `KeyChallenge` |
| Owner rotation | `POST .../rotation/disclosure`, `POST .../rotation` | delay / window / idempotency; start adds `step_up` | `SecurityAction` / `KeyChallenge` |
| Recover | `POST /v1/agents/{agent_id}/recover` | empty | `KeyChallenge` |
| Archive | `POST /v1/agents/{agent_id}/archive` | `confirm_name` | `Journey` kind `agent-retire` |

Archive stages: `agent.archive.stage.stopping-work`,
`agent.archive.stage.closing`.

`spec/layerx-platform/spec.kvx` records web agents task 12.4 as
`implemented` with a note that full runnable service wiring was
incomplete when recorded.

---

## Security and recovery

`SecurityActionKind`: `add-passkey`, `revoke-passkey`,
`revoke-session`, `revoke-all-sessions`, `add-authenticator`,
`disable-authenticator`, `rotate-backup-codes`,
`reveal-recovery-evidence`.

| Area | Operations |
| --- | --- |
| Digest | `POST /v1/security/actions` with `action`, optional `target_id` → `confirms` |
| Passkeys | `GET /v1/security/passkeys`; register begin/finish with `step_up`; revoke with `step_up` |
| Sessions | `POST /v1/security/sessions/{session_id}/revoke`, `.../revoke-all` with `step_up` |
| Authenticator | `GET /v1/security/authenticators`; setup begin/finish; disable; backup rotate |
| Recovery evidence | `POST /v1/security/recovery/evidence` with `evidence_id`, `step_up` → `TimedSecret` (`value`, `remask_at`, `copyable`) |

Onboarding recovery is a stage of the `onboarding` journey, not a
separate kind.

---

## Home, activity, support

| Surface | Operations |
| --- | --- |
| Home | `GET /v1/home` → `balance`, `agents`, `approvals`, `recent_activity`; `GET /v1/account/balance` |
| Activity | `POST /v1/activity/query`; `GET /v1/activity/{entry_id}` |
| Support | `POST /v1/support/conversations` with optional `topic` ∈ `deposit`, `withdrawal`, `agents`, `account`, `report` |

---

## Implementation status

`spec/layerx-platform/spec.kvx` marks human-api schema, onboarding,
binding, journeys, and most web journey tasks **done**. Production
KMS (task 5.8) and some settings/agent wiring are recorded as
**implemented** with remaining gaps. Where the spec says a path is
still pending, this page does not claim it is finished.

[Home](Home.md)
