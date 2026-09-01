# Threat model — LayerX Agent Interface

What this layer defends against, what it does not, and which mechanism carries
each defence. Protocol-level threats are modelled in
`spec/layerx-protocol/docs/threat-model.md`; this document covers the interaction
layer only.

---

## 1. Trust boundaries

```text
[ agent operator ]---keys--->[ signer: HSM / KMS / device ]
        |                             ^
        | policy, capabilities        | bytes + disclosure
        v                             |
[ application / LLM agent ]--->[ layerx-agentd ]--->[ LNI ]--->[ layerxd ]--->[ Paxeer ]
                                     |                              |
                                     +---local store (no authority)-+
```

| Party | Trusted for | Not trusted for |
|---|---|---|
| Agent operator | setting policy, provisioning keys | nothing at protocol level; the protocol checks anyway |
| Application / LLM agent | expressing intent | authority, amounts, counterparties, or any protocol fact |
| `layerx-agentd` | refusing, preparing, submitting, verifying | authorising, computing balances, asserting outcomes |
| `layerxd` | serving bytes and proofs | being honest — every answer is verified locally |
| Paxeer | custody, checkpoint registration, guarantees | ordinary activity semantics |

The central asymmetry: **the daemon is trusted to say no and never trusted to say
yes.** A local allow is the absence of a local objection, not authorisation.

---

## 2. Threats and mitigations

### T1 — A compromised daemon tries to move funds

*Attempt:* the daemon builds an activity the agent never asked for and submits it.

*Mitigation:* the daemon holds no agent primary key. Signing happens at the
signer, over bytes accompanied by a disclosure decoded from those same bytes; a
disclosure that does not re-encode to the bytes is a refusal. Where the daemon
holds a provisioned session key, its scope and expiry are protocol-enforced, so
the blast radius is exactly the delegated scope the state machine will honour —
not whatever the daemon believes.

*Residual:* a daemon holding a broadly scoped session key can act within that
scope. The mitigation is scope discipline at provisioning, which the capability
report makes visible.

### T2 — A compromised daemon lies about outcomes

*Attempt:* report a payment as settled that never executed, or a balance higher
than the ledger holds.

*Mitigation:* protocol values are carried as `Verified<T>` with a level
constructible only by a `layerx-proof` routine that performed the check, plus an
evidence record. The evidence export lets a counterparty re-verify the same facts
offline with `layerx-proof` alone. The qualification suite runs a hostile-layer
scenario and requires independent verification to catch the alteration.

### T3 — A hostile or buggy node

*Attempt:* altered balances, re-signed receipts, sub-threshold certificates,
truncated proofs, reordered events, withheld availability data.

*Mitigation:* every response is verified client-side before it is returned:
sequencer signature, merkle recomputation, state proof against a signed root,
distinct-signer threshold counting, DA chunk proofs and reassembly re-hashing. The
no-fabrication suite injects each of these and requires a verification failure or
an unavailability report — never the altered value surfaced as a result.

*Residual:* a node that withholds data can deny service. It cannot cause a false
fact to be reported; denial is visible as `Unavailable` plus an availability
failure record.

### T4 — Prompt injection through an LLM agent

*Attempt:* text in tool arguments, resource content or tool results instructs the
layer to widen scope, change an approval requirement, or redirect a counterparty.

*Mitigation:* every authority decision rests on data the daemon holds — session,
capability, policy, budget — never on model-supplied text. Arguments are validated
against the contract schema. Approval flows show the **disclosure decoded from the
prepared bytes**, not the model's request. Read-only deployments omit write tools
entirely rather than refusing them. An injection corpus tests each escalation as a
build gate.

### T5 — Replay and duplicate economic effect

*Attempt:* a retry, a duplicate delivery or a restart causes a second payment.

*Mitigation:* one caller intent maps to one protocol `idempotency_key`; retries
resend byte-identical bytes under that key; the protocol produces at most one
economic result. The daemon's own API idempotency returns the original result for
a repeat and a conflict for a differing body. The exactly-once suite drives
retries, concurrent duplicates, shedding and restarts and asserts one receipt per
intent.

### T6 — The ambiguous outcome

*Attempt:* a network failure at the moment of submission leads the layer to guess.

*Mitigation:* `Unknown` is a first-class state. Reservations stay held; resolution
happens only by receipt lookup; no heuristic infers the outcome from transport
behaviour. Reporting `Unknown` as success would risk delivering unpaid goods;
reporting it as failure would invite a double payment. Neither is permitted.

### T7 — Post-signature tampering

*Attempt:* alter amount, recipient or fee limit between signing and transmission.

*Mitigation:* the signature is verified against the exact bytes immediately before
transmission, and `layerx-client` transmits exactly the bytes it was given with no
re-encoding step anywhere in the path.

### T8 — Authority that outlives its revocation

*Attempt:* keep using a session whose underlying key was rotated or revoked.

*Mitigation:* the protocol authority is re-resolved from core state before every
write; revocation events from the stream invalidate sessions immediately and
cancel prepared-but-unsubmitted activities. The protocol enforces the same
independently, so a daemon that missed the event still cannot get the activity
executed.

### T9 — Cross-tenant leakage

*Attempt:* read another tenant's data, or infer its existence.

*Mitigation:* tenancy is in the storage key, not an edge filter; the tenant comes
from the authenticated principal, never from the request body; errors are
normalised so existence is not distinguishable; metric labels and traces are
scoped. The isolation suite attempts escape through every surface — API, SDKs, MCP
tools, subscriptions, exports, error paths — and treats a distinguishable
existence or timing signal as a build-breaking defect.

### T10 — Key exposure

*Attempt:* extract key material from logs, metrics, traces, panics or a keystore.

*Mitigation:* key material is zeroized on drop, excluded from every output
surface with a build gate and an output-scanning test, and stored under an
authenticated cipher with identity and network bound into the authenticated data.
Remote signers never export keys at all.

### T11 — Resource exhaustion and noisy neighbours

*Attempt:* a hot loop, retry storm or oversized message degrades everyone.

*Mitigation:* per-tenant limits and quotas, bounded queues with defined overflow,
outbound admission control that prioritises submission delivery and receipt
resolution, client-specific shedding, and explicit maximum message and page sizes.
No shedding decision may cause a duplicate economic effect.

### T12 — Malformed input reaching a decoder

*Attempt:* crash or hang the daemon with crafted bytes.

*Mitigation:* no panicking path reachable from decoding untrusted bytes; limits
enforced before allocation; fuzz targets for the codec, framing, contract surface
and policy loader, with panics, hangs and unbounded allocation treated as defects.

### T13 — Silent capability regression

*Attempt:* a node upgrade removes a capability and the layer papers over it.

*Mitigation:* the handshake recomputes the capability intersection on every
connection; missing capabilities fail dependent requests as `Unavailable` and
appear in the gap report, which is included in the qualification report.

---

## 3. Explicit non-goals

| Not defended | Why |
|---|---|
| A compromised signer | If the key holder signs, the protocol will honour it. Scope discipline and protocol-level limits bound the damage; nothing in this layer can overrule a valid signature. |
| A compromised operator | An operator can set permissive policy and provision broad scopes. The layer makes that visible and audited; it does not prevent it. |
| Node availability | The layer degrades honestly; it cannot make an unavailable node available. |
| Protocol-level attacks | Sequencer equivocation, guarantor collusion, DA withholding at the protocol level are the protocol's threat model. This layer detects and reports what it can verify. |
| Confidentiality of on-chain data | LayerX activity data is what it is; this layer does not add privacy guarantees. |

---

## 4. Assumptions

- The protocol conformance corpora are correct and current; divergence from the C
  core is a build failure here, so a wrong corpus would be caught as a
  disagreement rather than adopted silently.
- The published sequencer and guarantor key material is authentic and reaches the
  layer through a trusted configuration path.
- The host running `layerx-agentd` protects its local store and its process memory
  to the standard the deployment requires; the layer reduces what is worth stealing
  (no primary keys, no authority) but does not replace host security.
- Clock skew affects only local scheduling — retries, deadlines, freshness
  windows — never a verification decision, which reads no clock at all.
