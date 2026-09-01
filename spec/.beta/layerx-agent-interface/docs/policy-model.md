# Policy, capabilities and budgets

How the interaction layer restricts what an agent may do, and why every one of
these mechanisms can only refuse. Reference for `[req.9]`, `[req.10]`, `[req.11]`
and task groups 10, 11 and 12.

---

## 1. The negative-authority principle

The protocol decides authorisation. It decides it inside the deterministic state
machine, at execution time, from state — not from anything the daemon concluded
earlier. So a local control has exactly one useful power: to stop an activity
from being prepared or submitted at all.

```text
local allow  =  "no local objection"        (not authorisation)
local deny   =  "this will not be attempted" (a real, enforced outcome)
```

Every part of this document follows from that asymmetry. A capability that
appears to grant something is a bug in the capability model, because the grant
side is not the daemon's to give.

---

## 2. The four gates

A write request passes four gates before bytes are prepared. Each can refuse;
none can authorise.

| Gate | Question | Refusal type |
|---|---|---|
| Policy | Does any rule permit this, in this context? | `PolicyDenied` |
| Capability | Is this inside every dimension of the presented capability? | `CapabilityDenied` |
| Budget | Is there capacity under every applicable limit? | `BudgetExceeded` |
| Protocol authority | Does a currently valid protocol authority cover this? | `Unauthorized` |

The fourth gate is a pre-check of something the protocol will check again. It
exists to fail early and clearly, never to substitute for the protocol's own
evaluation.

---

## 3. Capabilities

### 3.1 Dimensions

A capability specifies **all** of:

- permitted activity types
- permitted counterparties
- permitted assets
- maximum single amount
- cumulative amount ceiling
- rate ceiling (count and value per window)
- permitted purpose constraints
- expiry

An unspecified dimension is not "unrestricted by default" — it is a rejected
capability. Defaults that open a dimension are how narrow-looking grants turn out
to be broad ones.

### 3.2 Narrowing is checked, not assumed

At creation, the underlying protocol authority is resolved and the capability is
rejected if it would permit an activity type, asset, counterparty or amount that
authority does not already permit. If the underlying authority is later narrowed,
capabilities that would now exceed it are disabled rather than left dangling.

### 3.3 Attenuation

Deriving a capability intersects every dimension with its parent. A derived
capability can never exceed its parent in any dimension. The derivation chain is
recorded, so any exercised capability is traceable to its root authority, and
revoking any node refuses the entire subtree — including prepared but unsubmitted
activities.

```text
root authority (protocol session key: asset.transfer, <= 1000 LXP, expires T)
  └── capability A  (asset.transfer, <= 100 LXP, counterparty in {X, Y})
        └── capability B  (asset.transfer, <= 10 LXP, counterparty {X})
              └── capability C  (rejected: named counterparty Z — not in parent)
```

### 3.4 Consumption

A ceiling is consumed by **verified receipts**, not by submission attempts. A
failed activity consumes nothing. The reservation decision is serialised per
ceiling, so concurrent uses cannot exceed it in aggregate, and a reservation is
held — not released — while an outcome is `Unknown`.

### 3.5 Where enforcement lives

Every restriction records whether it is protocol-enforced or daemon-enforced.
The capability report states this plainly, and for daemon-enforced restrictions
adds the sentence that matters: **bypassing the daemon bypasses this restriction.**
Describing a daemon-enforced restriction as a protocol guarantee is a
documentation-check failure, not a wording preference.

---

## 4. Budgets

### 4.1 Prefer the protocol

Where the protocol offers an enforceable equivalent — a budget, a capability
grant, a session key scope, a payer grant — the limit is created there, through an
ordinary prepared, signed and submitted activity. The response returns the
protocol object identifier and its receipt, so the limit's existence is evidence
rather than a daemon record.

This matters for a specific reason: a protocol budget still binds when the daemon
is down, bypassed, or compromised. A daemon-side counter does not.

### 4.2 Reconciliation

Local accounting is a cache. Consumption is derived from verified receipts and
protocol budget state; window boundaries and remaining allowance come from
protocol state rather than a local timer, so the daemon's view of a recurring
window matches the deterministic protocol view exactly.

When the local and protocol figures diverge:

- an explicit alert is raised, visible in health, not only in logs;
- the divergence is recorded in the audit trail with both figures, the last
  verified receipt and the head sequence;
- the **more restrictive** figure governs while the divergence is open;
- the local figure is never silently adopted.

### 4.3 Reservations

```text
prepare ──► reserve ──► submit ──► terminal? ──► release
                 │                     │
                 │                 unknown ──► HOLD (no release, no reuse)
                 └── refused ──► release immediately, leave nothing behind
```

Reservations are persisted. On restart, spend accounting is rebuilt from
persisted receipts and protocol state **before** any write is accepted against
that limit, and held-but-unresolved amounts are reported distinctly from consumed
amounts so an operator can see why capacity is unavailable.

---

## 5. Policy

### 5.1 Deny by default

No rule permitting the request means the request is denied. A request is never
allowed because nothing denied it.

### 5.2 Determinism

Evaluation is a function of the request, the session, the capability, the
reconciled budget state and the loaded policy version. It reads no wall clock for
convenience, no randomness, no load signal, and does not depend on map iteration
order. Identical inputs yield identical decisions, which is what makes dry-run
meaningful and audit reconstruction possible.

### 5.3 Fail closed

Evaluation failure, timeout or internal error denies the request. A policy engine
that fails open is not a control; it is a control-shaped delay.

### 5.4 Vocabulary

Constraints may name activity type, counterparty, asset, amount, cumulative rate,
purpose, capability, session, agent, tenant, time window and required approval.
Thresholds are exact integers — a floating-point threshold on a monetary control
is a rounding bug waiting for the boundary case.

### 5.5 Explanation and dry-run

Every decision records the policy version, every rule that matched, the deciding
rule and the reason, and that record is retrievable for the audit retention
window. Dry-run returns the same decision and explanation with no side effect
beyond its audit entry, so a policy change can be evaluated against recorded
request corpora before activation.

### 5.6 Approval holds

A rule may require human or external approval. The request enters an explicit
awaiting-approval state; the approver is shown the **disclosure decoded from the
prepared bytes**, not the caller's request; and the hold expires deterministically
if approval does not arrive. Expiry never auto-approves.

### 5.7 Versioning

Policy sets are versioned, validated before activation, and applied only to
requests received after activation. An invalid set is rejected while the previous
version continues. Prior versions are retained so an audit entry naming version N
can be reconstructed after version N+1 is live.

---

## 6. Worked example

An operator delegates a research sub-agent a small, bounded spending ability.

```text
1. Operator provisions a protocol session key to the daemon:
     activity types: asset.transfer
     expiry:         30 days
     revocation seq: recorded in protocol state
   -> enforced by the LayerX state machine

2. Operator creates a protocol budget through a signed activity:
     per-period limit: 500 LXP / 24h
     subaccount:       agent:<did>:budget:research
   -> enforced by the LayerX state machine; receipt returned

3. Operator creates a daemon capability, attenuating that authority:
     activity types:  asset.transfer
     counterparties:  {data-provider-A, data-provider-B}
     max single:      25 LXP
     rate:            20 transfers / hour
     purpose:         "research-data"
     expiry:          7 days
   -> daemon-enforced; report states plainly that bypassing the daemon
      bypasses the counterparty, rate and purpose constraints

4. Policy requires approval above 100 LXP cumulative per day.
   -> daemon-enforced; approval shows the disclosure, expires deterministically
```

What survives a compromised daemon: the session key scope and expiry, and the
500 LXP per-day budget. What does not: the counterparty allowlist, the per-item
ceiling, the rate limit and the approval requirement. The report says so, in those
terms, rather than presenting four controls as though they were equally binding.
