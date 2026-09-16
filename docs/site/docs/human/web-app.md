# Human web application

The platform specification (`spec/layerx-platform/spec.kvx`, decisions
`two_shells` and `v1_scope`) requires one Next.js application with a public
`/explorer` plane and an authenticated `/app` plane, plus mobile and desktop
shells that share state and journey logic.

The application lives at `human/apps/web`. It must use the owner-supplied
`@layerx/ui` package. The browser never assembles protocol payloads and never
receives LayerX private key material. The TypeScript client is generated from
`human/schema/human-api`.

Hosted Human material and KMS seal handling are
[Hosted Human](hosted.md). Custody credit at the protocol boundary is
[Custody](custody.md). Journeys are [Journeys](journeys.md).

## Planes in this tree

Authenticated `/app` routes present in `human/apps/web/src/app/app/` include
home, deposit, move, withdraw, agents, approvals, notifications, activity,
settings (including wallet, security, devices, and exit), support, journeys,
and recovery. Public `/explorer` routes include the explorer home, accounts,
batches, checkpoints, programs, receipts, and verify.

Those paths are source-tree routes. They are not a claim that a public
hostname is deployed.

## What the plane may not do

Requirement 1 of `spec/layerx-platform/spec.kvx` requires the human plane to
change protocol state only by compiling a typed intent through
`layerx-intents`, preparing through the agent layer, obtaining a
disclosure-bound signature from the custody service, and submitting through
the agent layer. Success or Done is rendered only from a verified LayerX
receipt or a verified Paxeer finality proof. Unknown outcomes are
still-checking.

The default authenticated surfaces expose exactly five ideas: log in, add
money, move money, manage agents, see what happened (requirement 2). Protocol
words such as DID, checkpoint, and idempotency are banned on those default
surfaces. Technical details and the public explorer are exempt.

## Hosted wiring

The Human HTTPS API is an in-cluster Service `layerx-human` on the node pod's
`human` container. The disposable cluster forwards it on host port `19453`
([Hosted gateway](../platform/hosted-gateway.md),
[Testnet quickstart](../overview/quickstart.md)).
That origin is a cluster port-forward, not a public product URL.

## Status

The web application source and hosted Human provisioning exist. The platform
specification's usability, visual-regression, and performance gates are named
as polish and are not required for the beta
(`spec/layerx-beta/spec.kvx`, decision `polish_boundary`). Production
certification of the hosted human plane is outside the beta bar.
