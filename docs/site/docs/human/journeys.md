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
([Hosted control](../operators/testnet-control.md),
[Quickstart](../overview/quickstart.md)).
`/v1/journeys/settlement` is not one of them.

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 7) requires
per-journey readiness: a journey is ready only when every declared dependency
is reachable. A global green with a missing dependency is a defect.

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
