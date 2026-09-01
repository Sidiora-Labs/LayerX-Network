# Source brief — LayerX Human Interface

This document is the provenance record for `spec/layerx-human-interface/spec.kvx`.
Every normative statement in that file traces back to the planning brief captured
here, to the protocol feature it sits on top of (`spec/layerx-protocol/`), to the
agent layer it consumes (`spec/layerx-agent-interface/`), or to a locked decision
recorded in `[decision]`.

---

## 1. The request

Build the Human Control Plane for LayerX: the surface through which people —
not autonomous agents — hold accounts, fund them from Paxeer, create and govern
managed agents, approve or veto agent spending, move money, and see what
happened. It comprises:

- A custody-aware backend (`layerx-human-service`) that holds human and managed
  agent DID keys in KMS-backed custody and signs only through the
  disclosure-bound signing path the agent layer already defines.
- A typed-intent compiler (`layerx-intents`) so no human-facing component ever
  assembles raw protocol payload bytes.
- One additive extension to the agent layer contract: an `approval.*` operation
  module. Typed intents reach the core through the agent layer's existing
  prepare / sign / submit pipeline.
- A Paxeer custody-boundary client (`layerx-paxeer-client`) for deposits,
  withdrawal claims and emergency exits.
- A public explorer index (`layerx-explorer-index`) as a rebuildable projection
  over the node boundary.
- One Next.js web application with two native shells — a mobile experience and a
  desktop experience that share state and journey logic but ship device-specific
  layouts and components — split into a public `/explorer` plane and an
  authenticated `/app` plane.

## 2. The critical rules

> The surface exposes exactly five ideas: log in, add money, move money, create
> and manage an Agent, see what happened. Everything else — DIDs, session keys,
> capabilities, proofs, checkpoints, routes, retries — is engine room.

> "Done" is only ever rendered when a verified LayerX receipt or a Paxeer
> finality proof backs it. The human plane inherits the agent layer's
> non-authority rule wholesale: it never invents protocol state and never
> asserts an outcome the core did not produce.

Two obligations fall out of the second rule, exactly as in the agent layer:
every mutation becomes a typed intent, compiled to canonical bytes, disclosed,
signed by the custody service, and submitted through the agent layer; and every
claimed result is receipt- or proof-backed, with unknown outcomes reported as
"still checking" and resolved only by receipt lookup.

## 3. Component layout

```text
human/
  schema/human-api/        # versioned human-facing contract (HTTP+JSON), TS SDK generated
  crates/
    layerx-human-service   # auth (passkeys), KMS custody, journeys, approvals, notifications
    layerx-intents         # typed intent -> canonical module payload compiler (THE only builder)
    layerx-paxeer-client   # Paxeer reads, custody finality, deposit proofs, claim construction
    layerx-explorer-index  # rebuildable public projection over the node boundary
  apps/web                 # Next.js app: /explorer (public, SSR) + /app (authenticated)
```

The Rust crates reuse `layerx-types`, `layerx-wire`, `layerx-crypto`,
`layerx-proof`, `layerx-client` and `layerx-sdk` directly. Nothing is
re-implemented, and the browser speaks `human-api` only — it never assembles
payload bytes and never holds key material.

## 4. The identity contract

```text
Human login principal (passkey)
│ verified association
▼
Human LayerX DID + main account ── bound payout address ── Paxeer EVM wallet
│
├── recovery authority over managed Agent DIDs
├── protocol budgets and capability grants
└── funding relationships with Agent accounts
```

Human authentication, LayerX authority, and Paxeer wallet control remain
distinct. The EVM binding is a payout/ownership association only; by protocol
design (`layerx-protocol` req 3, ac 9) it grants the EVM key no authority to
move LayerX funds. Account creation is complete only when the LayerX receipt
for DID activation verifies.

## 5. The money movement model

"Deposit" and "withdrawal" are reserved for the Paxeer custody boundary.
Human-to-agent and agent-to-human movements are "fund", "allocate", "return"
or "transfer" — surfaced in the UI as one verb, "Move money".

| User operation | Protocol mechanism | Completion condition |
|---|---|---|
| Paxeer wallet → Human account | Custody tx, finalised proof, then `bridge.deposit_credit` | Verified LayerX credit receipt |
| Human → Agent account | Authenticated 402LXP `SEND` | Verified LayerX receipt |
| Human → managed Agent budget | Create/fund a protocol budget subaccount | Verified budget receipt |
| Agent → Human account | Agent-authorised `SEND`, `RECEIVE` under a payer grant, or budget defunding | Verified LayerX receipt |
| Human → Paxeer wallet | `bridge.withdraw_request`, checkpoint proof, Paxeer claim | Finalised Paxeer payout tx |
| Emergency exit | Proof against last finalised checkpoint | Paxeer exit tx finalised |

Human reclaim over Agent funds exists only via protocol budgets or explicit
payer grants — never a silent key-based sweep.

## 6. The UI mandate

The user's direction, verbatim requirements:

1. A desktop-native and a mobile-native experience live in the same app, with
   mobile-specific components and layouts and desktop-specific components and
   layouts.
2. The app follows every professional standard expected of an app of this
   caliber serving 1M+ users.
3. Every error gets a user-friendly screen with actionable buttons (Retry,
   Reload, Report, and the like).

The visual language derives from a survey of all 292 screens of the user's
previous app's Figma set (19-batch structured review; synthesis published as the
"LayerX Human Interface" UI draft artifact, 2026-08-15, reviewed and accepted).
The owner-supplied `@layerx/ui` package is the exact visual reference for the
application: its component API, borders, dividers, shadows, gradients, palette,
radii, typography, interaction states and responsive treatments are retained.
The product requirements add the missing loading/offline/error state matrix,
distinguished destructive confirms, secret handling, complete external
hand-offs and versioned copy without replacing the package's visual language.

## 7. What upstream features already fix

| Inherited | Source |
|---|---|
| DID identity, rotation, recovery authority, challenge delay | `layerx-protocol` req 3 |
| EVM payout binding grants no LayerX authority | `layerx-protocol` req 3 ac 9 |
| 402LXP as the single financial doorway | `layerx-protocol` reqs 7–9 |
| Budget subaccounts and spend control | `layerx-protocol` req 16 |
| Bridge deposits, withdrawals, challenge windows, exits | `layerx-protocol` req 24 |
| Prepare / disclose / sign / submit / track pipeline | `layerx-agent-interface` reqs 12–13 |
| Disclosure-bound signing and remote signers | `layerx-agent-interface` req 5 |
| Verified reads, proofs, verification levels | `layerx-agent-interface` reqs 6, 14 |
| Approval holds with deterministic expiry | `layerx-agent-interface` req 11 ac 8, task 12.4 |
| Tenant isolation, audit, rate limits | `layerx-agent-interface` reqs 19–21 |

Where this spec restates one of those, it restates it as a consumer obligation.
If the two ever disagree, the upstream spec wins and this spec is the defect.

## 8. Decisions locked before implementation

- **Custodial by default.** Passkey-backed login; DID keys live in the KMS-backed
  custody service. Key export / self-custody is out of scope for v1. This is the
  honest price of "a five-year-old can use it", made acceptable by the
  protocol's recovery, rotation and revocation model.
- **The seam lands first.** `approval.*` is an additive agent-layer change
  (contract v1.x) built in the first wave. Typed intents live in the human
  workspace and use the agent layer's existing prepare / sign / submit
  pipeline — the agent-interface spec's scope and qualification gate stay
  clean.
- **The wallet opens only at custody-boundary signing moments** — the binding
  statement, the deposit, or the claim — exactly once per required signature.
  It is never a login method and never a signer for LayerX activities.
- **v1 scope.** In: onboarding, wallet binding, agent lifecycle, deposits,
  internal moves, withdrawals, emergency exit, approvals, notifications, public
  explorer, activity view, audit export, support chat. Out (post-v1): escrow /
  streams / services / perps workspaces, team roles beyond owner, address book,
  localization beyond copy-catalog readiness, self-custody key export.
- **Next.js, one app.** Two shells selected SSR-safely by viewport and pointer
  capability; tablets get the desktop layout with touch targets.
