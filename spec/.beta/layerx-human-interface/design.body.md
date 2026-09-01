# LayerX Human Interface — Design

The Human Interface is the control plane people touch. It provides passkey-backed
accounts with custody-held LayerX identities; deposits from and withdrawals to
Paxeer; internal money movement; managed-agent creation, funding, limiting,
pausing and retirement; an approval inbox with human veto over held agent
spending; one activity feed with receipts behind everything; a public explorer;
and a web application with two native shells.

It is Rust services plus a TypeScript web application, lives in its own workspace
at `human/`, and is governed by two rules every other decision serves:

> **The surface exposes exactly five ideas** — log in, add money, move money,
> manage agents, see what happened. Everything protocol-shaped is engine room.

> **"Done" is receipt-backed, always.** No surface renders success for a money
> movement or protocol mutation unless a verified LayerX receipt or a Paxeer
> finality proof backs it. Unknown is "still checking", never success, never
> failure.

The protocol specification is normative for protocol behaviour and the
agent-interface specification is normative for the boundary and the agent layer.
Where this document restates either, it restates a consumer obligation. If they
disagree, the upstream spec is right and this plane has a defect.

---

## 1. What this plane is, and what it is not

| It is | It is not |
|---|---|
| A custodian of keys people never see | A wallet the user operates key-by-key |
| A compiler of typed intents | An assembler of raw payload bytes |
| A renderer of verified receipts | A source of claimed outcomes |
| A veto surface over agent spending | A grant of authority policy could not refuse |
| A client of the agent layer | A second path to the core |
| A translator of protocol states into plain words | A vocabulary lesson for end users |

The plane inherits the agent layer's non-authority rule wholesale, and adds one
of its own: no protocol vocabulary on default surfaces. Both are enforced by
gates, not convention.

## 2. The identity contract

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

Three authorities, kept distinct:

- **The application identity** (passkey) authenticates a person to this plane.
  It signs nothing on the protocol.
- **The LayerX DID and its keys** hold protocol authority. They are generated
  in and never leave the KMS-backed custody keystore. The custody service signs
  exclusively through the disclosure-bound path: exact canonical bytes plus a
  disclosure that must re-encode byte-identically, or refusal.
- **The Paxeer EVM wallet** owns the custody boundary: it signs the binding
  statement, deposits, withdrawal claims and exits — and nothing else. Per the
  protocol, its binding grants it no authority over LayerX balances, and this
  plane never treats an EVM signature as LayerX authorisation.

Account creation completes when the DID registration receipt verifies. Recovery
authority is pre-registered during onboarding; the protocol's challenge delay
applies when recovery or rotation is later exercised. Managed agents get their
own DIDs, with the human as recovery authority,
operating through session keys provisioned to `layerx-agentd` with explicit
expiry and scope.

## 3. Component boundary

```text
human/
  schema/human-api/        # versioned HTTPS+JSON contract; TS client generated
  crates/
    layerx-human-service   # auth, custody, journeys, approvals, notifications
    layerx-intents         # typed intent -> canonical payload compiler
    layerx-paxeer-client   # custody finality, deposit proofs, claims, exits
    layerx-explorer-index  # rebuildable public projection over the node boundary
  apps/web                 # Next.js: /explorer (public, SSR) + /app (authed)
```

The crates consume `layerx-types`, `layerx-wire`, `layerx-crypto`,
`layerx-proof`, `layerx-client` and `layerx-sdk` as path dependencies. Three
structural rules, each with a CI gate:

1. **`layerx-intents` is the single payload authority.** No other `human/`
   component may invoke `layerx-wire` encoding entry points. The browser never
   sees payload bytes at all.
2. **The agent layer is the only path to the core.** The human plane inherits
   every boundary prohibition — no SQLite, no node files, no C struct layouts.
3. **The browser holds no keys.** The web application authenticates people and
   renders journeys; the custody service signs. The bundle is scanned to prove
   no signing capability ships to the client.

Write path, end to end: typed intent → `layerx-intents` compiles canonical
payload → agent layer `prepare` returns the activity and its disclosure →
disclosure checked field-for-field against the originating intent → custody
signer signs the exact bytes (step-up where designated) → agent layer submits →
receipt verified → only then does any surface say "Done".

## 4. The money movement model

"Deposit" and "withdraw" are custody-boundary words. Inside LayerX the plane
funds, allocates, returns and transfers — surfaced as one verb, **Move money**,
with the route resolver picking the mechanism:

| User operation | Protocol mechanism | Completion condition |
|---|---|---|
| Wallet → Human account | Custody tx, finalised proof, `bridge.deposit_credit` | Verified credit receipt |
| Human → Agent account | Authenticated 402LXP `SEND` | Verified receipt |
| Human → Agent budget | Budget create / fund | Verified budget receipt |
| Agent → Human | Budget defund, agent-authorised `SEND`, or `RECEIVE` under a payer grant | Verified receipt |
| Human → Wallet | `bridge.withdraw_request`, checkpoint proof, claim | Finalised Paxeer payout |
| Emergency exit | Proof against last finalised checkpoint | Finalised Paxeer exit |

Journeys are durable state machines: every leg is receipt-verified before the
next starts, every journey resumes across restarts without duplicating a leg,
and an unknown leg pauses the journey in "still checking" with every
duplicate-capable control locked. Reclaim from agents uses budget defunding or
explicit grants only — the custody service holding an agent's keys is never a
license to sweep its account.

## 5. Approvals — the human veto

The agent layer gains one additive contract module: `approval.*` (list, get,
approve, reject), carrying each policy hold's structured disclosure and the
digest of its exact canonical bytes. The inbox renders approvals **only** from
the disclosure that re-encodes to the held digest — never a paraphrase.
Approving requires a step-up passkey ceremony bound to that digest and applies
to exactly that digest; expiry is deterministic; expired holds are not
approvable; concurrent decisions converge on one outcome. Approvals are a
daemon-enforced restriction and are labelled as such everywhere.

## 6. One application, two native shells

One Next.js app, two planes, two shells. The shells share state, data hooks,
journey logic and the copy catalog; layouts and components are device-specific:

| Pattern | Mobile shell | Desktop shell |
|---|---|---|
| Navigation | Bottom tab bar: Home, Agents, Activity, More | Left sidebar with approval badge, Explorer link |
| Primary action | Full-width pinned pill at thumb reach | Fixed-width button in pane footer / header |
| Confirmation | Bottom sheet with consequence copy | Centered modal ≤440px, Esc + overlay dismiss |
| Detail / education | Bottom sheet or pushed screen | Right drawer or inline expansion |
| Filters | Sheet with Clear + Apply | Popovers anchored to chips; calendar range |
| Money lists | Stacked rows, month bands with subtotals | Sortable table, hover states, sticky groups |
| Multi-step journeys | Full-screen wizard, one decision per screen | Split pane: form left, live summary right |
| Search | Search screen from header | Command bar, Cmd+K |
| Code / secret entry | Tap-per-box code kit | Segmented input with full-code paste |
| Notifications | Pushed screen, recency segments | Bell popover + archive page |

Shell selection is SSR-safe — viewport and pointer capability, confirmed
client-side, corrected without thrash; never user-agent alone. Tablets get the
desktop layout with touch targets. Both shells have full journey parity and both
are exercised by the qualification suite.

## 7. The design system

- **The owner-supplied component library is authoritative.** Both shells use
  the exact `@layerx/ui` component API and styling, including its borders,
  dividers, shadows, gradients, layered surfaces, radii and control states.
  Application code does not create a competing primitive or restyle a package
  primitive.
- **The package token stylesheet is canonical.** Palette, typography, radii,
  elevation and motion come from `@layerx/ui/styles.css`; the application
  composes those tokens without silently redefining the visual contract.
- **Semantic color follows the package.** Accent, success, destructive and
  warning treatments use the matching library variants.
  Money renders sign + color + word, tabular numerals, one amount format, one
  date format.
- **Confirmation grammar.** Reversible: neutral primary with a consequence
  sentence. Destructive: red primary. Irreversible: red primary plus typed
  confirmation. The kit makes the wrong grammar unrepresentable.
- **Fee math is disclosed, numbered.** Where a charge needs explanation, the
  app walks it in numbered plain-language steps from inputs to total — the
  carried-over fee-math disclosure pattern.
- **Motion** uses the package durations and easing, honors
  `prefers-reduced-motion`, and uses the package skeleton treatment for loading.

## 8. The status translation table (normative)

| Protocol truth | User-facing state | Backed by |
|---|---|---|
| Prepared, unsigned | Getting ready | Local journey state |
| Submitted, no receipt | Sending | Submission record |
| Executed, awaiting checkpoint | Processing | Verified receipt |
| Receipt verified | **Done** | Verified LayerX receipt — the only source of "Done" |
| Checkpoint finalised | Done, finalised | Checkpoint proof |
| Outcome unknown | Still checking — don't send again | Resolved only by receipt lookup; duplicating controls locked |
| Refused / failed | Didn't go through — with whether money left | Typed refusal |
| Held for approval | Waiting for you | Approval hold, deterministic expiry |
| Deposit stages | Waiting for wallet → Confirming on Paxeer → Crediting → Done | Wallet ack, custody finality, credit receipt |
| Withdrawal stages | Processing → Waiting for settlement → Ready to claim → Paid out | Debit receipt, checkpoint proof, Paxeer finality |

A status string outside this table on a default surface is a build failure. The
table lives in the copy catalog as its first normative entries.

## 9. Nothing fails silently

Every screen ships a state matrix: loading (layout-reserving skeletons), empty
(headline + one sentence + CTA), error, offline, degraded, and still-checking
where money is in flight — enumerated by a registry the qualification suite
checks. Every error state follows one anatomy: what happened, what it means for
the user's money, actionable buttons (Retry where retriable, Reload where
structural, Report always), and a collapsed technical section with machine code
and trace identifier. Error boundaries wrap every route; a blank screen or raw
stack trace is a defect. External hand-offs (the wallet, file surfaces) always
have in-progress, failure and cancel states. Staleness is stated with age and
reference point, in amber. The trace identifier travels browser → service →
agent layer and back, and is what the Report button submits.

## 10. The public explorer

`/explorer` is server-rendered, unauthenticated, and shows protocol-public data
only: checkpoints, batches, receipt lookup, account views, and an evidence
verifier that verifies pasted bytes with the plane's own proof machinery and
reports the achieved verification level. It is served from
`layerx-explorer-index`, a projection fed exclusively through the node boundary
and rebuildable from it, stating its own freshness on every page. Every
"Technical details" link in the app resolves here, signed out. The explorer is
also the independent check on this plane: exported evidence verifies against it
without trusting `layerx-human-service`.

## 11. Operational posture

Every stored row is principal-scoped; human principals map to agent-layer
tenancies so isolation composes. An append-only hash-chained audit log covers
authentication, signing decisions, approvals, journey transitions, security
changes and notification dispatches, exportable per user with its evidence.
Telemetry carries no PII, balances or addresses, enforced by a redaction layer
with a CI gate. Health surfaces distinguish plane, agent-layer and Paxeer-side
degradation, and the app translates each honestly.

## 12. Qualification

Release is gated on: every journey end to end against a real node, a real
`layerx-agentd` and a Paxeer test network, in both shells; a hostile-plane suite
proving a tampered service is caught by evidence verification and that the UI
cannot render "Done" without a verifying receipt; a fault-injection matrix
proving exactly-once economic effect per leg; the state-matrix, visual, copy,
UI-rule and accessibility gates; performance budgets and a restart-spanning
soak; a usability gate with real participants completing the core journeys on
mobile without documentation; and a qualification report that tooling refuses
to release past while any gate is unmet.
