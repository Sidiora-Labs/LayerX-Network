# Human journeys

The platform specification (`spec/layerx-platform/spec.kvx`) names the v1
journeys: onboarding, wallet binding, managed-agent lifecycle, deposits,
internal movements, withdrawals with claims, emergency exit, the approval
inbox, notifications, the public explorer, the activity view, audit export,
and support chat. Escrow, streams, services, and perps workspaces are
post-v1.

The application source under `human/apps/web/src/app/app/` contains routes
for deposit, move, withdraw, agents, approvals, notifications, activity,
settings, exit, support, journeys, and recovery. Those files are the
implementation surface. This page does not invent additional HTTP APIs.

## Money vocabulary

Decision `vocabulary` in `spec/layerx-platform/spec.kvx`: deposit and
withdrawal name movements across the Paxeer custody boundary only. Movements
inside LayerX are fund, allocate, return, or transfer, surfaced as one verb:
Move money. The route resolver picks the protocol mechanism. The user never
selects a transfer type.

## Done is a receipt

No surface renders success for a money movement or protocol mutation unless a
locally verified LayerX receipt or a verified Paxeer finality proof backs it.
Unknown outcomes are still-checking and are resolved only by receipt lookup
under the idempotency key. While unresolved, controls that could duplicate
the economic effect stay disabled.

## Hosted journeys

Hosted control admits four developer journeys:
`/v1/journeys/funding`, `/v1/journeys/payment`,
`/v1/journeys/receipt-inspection`, and `/v1/journeys/programs`
([Hosted control](../operators/beta-control.md),
[Quickstart](../overview/quickstart.md)).
`/v1/journeys/settlement` is not one of them.

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 7) requires
per-journey readiness: a journey is ready only when every declared dependency
is reachable. A global green with a missing dependency is a defect.

## Stating an intent once

A human states one intent — move an amount of one asset from one endpoint to
another, where either end may be the bound Paxeer wallet or a LayerX account,
agent or agent budget — and the network plans it. The plan is a single ordered
set of legs spanning both domains, with one digest bound into every action key,
every signing context and the journey idempotency key. The user signs that
plan; a plan whose legs changed no longer matches its signatures.

This does not add a transfer type. Deposit and withdrawal still name movements
across the Paxeer custody boundary only, everything inside LayerX is still one
verb, and the route resolver still picks the mechanism. Intent planning is how
one statement reaches a route across both domains at once, not a new choice put
to the user.

The planner refuses rather than guesses. If the wallet binding, a balance, the
allowance inventory or the fee schedule cannot be read, the answer is a typed
refusal naming that source — never a plan over a partial state. Progress is
read through the existing journey status route, and done is still a receipt.

Planning happens at `POST /v1/intents/plan`, which changes nothing and answers
with the legs, the total fee, the plan digest and the exact list of what must
be signed. Submitting the signed plan at `POST /v1/intents/submit` re-plans the
same intent against the state as it stands at that moment and refuses when the
recomputed digest no longer matches the one that was signed, so a plan that was
agreed against a state which has since moved is refused rather than executed
against a state the human never saw.

## Reclaim

A human takes money back from a managed agent only by defunding a protocol
budget, by an agent-authorised transfer, or by drawing a receive under an
explicit payer grant. There is no key-based sweep of an agent account, even
though the custody service holds the agent's keys
(`spec/layerx-platform/spec.kvx`, decision `reclaim`).

## Related pages

- [Web application](web-app.md)
- [Custody](custody.md)
- [Owner registration](owner-registration.md)
- [Hosted Human](hosted.md)
- [Approvals in the agent contract](../agents/agentd.md)
