# Welcome to LayerX Network

This is the documentation site for LayerX Network. Normative text lives in the
specifications under [`spec/`](https://github.com/Sidiora-Labs/LayerX-Network/tree/main/spec).
Where a surface is specified but not yet present in this tree, the page says so.

LayerX Network is a deterministic execution and accounting network built for autonomous agents.

The public testnet exposes a gateway API and a faucet. There is no LayerX mainnet. Custody and settlement live on Paxeer. LayerX is licensed under the Apache License, Version 2.0.

Ordinary agent activity is executed and ordered inside LayerX. Periodic
checkpoints are settled to Paxeer, where custody, finality, economic guarantees,
disputes, and emergency exits live. This separation keeps ordinary execution
inside LayerX while custody and checkpoint settlement remain on Paxeer.

## Repository and monorepo structure

This is the canonical Sidiora Labs monorepo for LayerX Network and the Paxeer Network. The Paxeer settlement node source lives under the repository root (`go.mod`, `chain.mk`, `daemon/`, `node/`, `modules/`, `consensus/`, `sdk/`, `rpc/`) with independent build, release tags (`paxeer-network/vX.Y.Z`), and trust boundaries. Co-location keeps the protocol, settlement network, and their automation auditable in one place while preserving separate deployment authority.

LayerX Programs is kernel module ID `9` (`LXP_MODULE_PROGRAMS` in `include/layerx/lxp_module.h`). Guest code runs in that module's namespace. Every monetary effect is forced through 402LXP; no program ever holds direct balance-writing authority. See [Programs](programs/index.md). Protocol 3 (`LXP_PROTOCOL_VERSION_STATE_COMMITMENT`) is the beta wire; the C header default `LXP_PROTOCOL_VERSION` remains occupancy protocol 2 (`include/layerx/lxp_protocol.h`).

## What is Paxeer?

Paxeer handles custody, checkpoint registration, guarantor bonds, challenges,
withdrawals, and emergency exits. LayerX registers periodic checkpoints with
Paxeer rather than representing each ordinary activity as a Paxeer transaction.

## Key properties

Three rules sit at the center of LayerX:

1. **One canonical history.** Every accepted or failed activity receives a global sequence. State roots are chained per activity, not only per batch.
2. **One financial doorway.** `402LXP` is the only component allowed to write balances. Protocol modules produce validated transfer sets rather than mutating funds themselves.
3. **One reproducible result.** Consensus-critical execution excludes floating point, local clock decisions, database iteration order, and other sources of nondeterminism.

## What LayerX supports

| Area | Responsibilities |
| --- | --- |
| Identity and authority | Agent DIDs, primary and session keys, scoped capability grants, rotation, recovery, revocation, and expiry |
| Money movement | Authenticated sends and receives, asset accounts, deposits, withdrawals, and reserve accounting |
| Spending controls | Holds, escrow, recurring budgets, delegated limits, approvals, and metered streams |
| Agent commerce | Offers, commitments, tool-execution attestations, delivery, acceptance, and disputes |
| Markets | Oracle intake, order books, positions, funding, margin, liquidation, and insurance accounting |
| Network operation | Sequencing, replicas, batch construction, data availability, replay, fees, and metering |
| Settlement | Guarantor attestations, checkpoint registration, custody reconciliation, claims, and emergency exits |

## Fees

Fees are computed from the committed canonical schedule.
Asset fee prices are named for ordinals `1`, `4`, `5`, `6`, `7`, `8`, `10`, and
`11`; see [Assets and tokens](concepts/assets.md#named-fee-schedule). Public estimation
returns the schedule and snapshot that produced the value, and does not reserve
the fee or prove execution.

See `docs/MONOREPO.md` for build boundaries, workflow naming, and tag conventions.

## Resources

- [Testnet cluster quickstart](overview/quickstart.md)
- [Getting started on testnet](overview/getting-started.md)
- [Payments developer path](overview/payments.md)
- [Public JSON-RPC](platform/gateway-rpc.md)
- [Public payment API transcript](platform/public-api.md)
- [Assets and tokens](concepts/assets.md)
- [Commitment levels](protocol/commitment-levels.md)
- [Running an agent](agents/running.md)
- [Protocol](protocol/index.md)
- [Modules](protocol/modules.md)
- [Finality](protocol/finality.md)
- [Custody](human/custody.md)
- [Protocol design](https://github.com/Sidiora-Labs/LayerX-Network/blob/main/spec/layerx-protocol/design.md)
- [Contributing guide](https://github.com/Sidiora-Labs/LayerX-Network/blob/main/CONTRIBUTING.md)
- [Security policy](https://github.com/Sidiora-Labs/LayerX-Network/blob/main/SECURITY.md)
- [Qualification documentation](operators/qualification.md)
- [Monorepo layout](overview/monorepo.md)
- [Programs](programs/index.md)
- [Sandbox](programs/sandbox.md)
- [Storage scan](programs/storage-scan.md)
- [SDK terminal verification](agents/sdk-verification.md)
- [Portable receipt verifier](interop/portable-receipts.md)
- [x402 transport](interop/x402.md)
- [402LXP protocol](https://github.com/Sidiora-Labs/LayerX-Network/blob/main/spec/402lxp/protocol.md)
- [402LXP RPC verification guide](https://github.com/Sidiora-Labs/LayerX-Network/blob/main/spec/402lxp/README.md)
- [Agentd](agents/agentd.md)
- [CLI](platform/cli.md)
- [Hosted core](platform/hosted-core.md)
- [Hosted authority](platform/hosted-authority.md)
- [Programs workspace gates](programs/workspace.md)
- [Porting](operators/porting.md)
- [Beta cluster](operators/beta-cluster.md)
- [Hosted gateway](platform/hosted-gateway.md)
- [Hosted webhooks](platform/webhooks.md)
- [Hosted registry](platform/registry.md)
- [Registry deployment journal](platform/registry-journal.md)
- [Hosted Human](human/hosted.md)
- [Hosted internal](operators/hosted-internal.md)
- [Hosted identity](platform/identity.md)
- [Hosted faucet](platform/faucet.md)
- [Hosted testnet control](operators/testnet-control.md)
- [Hosted node](platform/hosted-node.md)
- [Hosted agent boundary](platform/agent-boundary.md)
- [Paxeer boundary](concepts/paxeer-boundary.md)

---

LayerX Network is developed by [Sidiora Labs](https://github.com/Sidiora-Labs).
