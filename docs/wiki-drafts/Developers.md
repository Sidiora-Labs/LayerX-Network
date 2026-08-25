<!-- Wiki draft for https://github.com/Sidiora-Labs/LayerX-Protocol/wiki/Developers
     Andrew copies this after merge. Wiki has no PR flow. -->

# Developers

Run from source. Limited beta opens September 7. Source is open for inspection while we qualify the public lane.

There is no public LayerX RPC to point an agent at yet. The node, SDKs, MCP tools, interop transports, and the Paxeer Network tree are in this monorepo today.

This wiki is LayerX Protocol by Sidiora Labs (LXP1), settling on Paxeer Network (EVM chain ID `125`). The repository is [Sidiora-Labs/LayerX-Protocol](https://github.com/Sidiora-Labs/LayerX-Protocol).

---

## Five ways in

| Path | What you get | Where it lives |
| --- | --- | --- |
| Node | Run the protocol locally | `cmd/layerxd`, `cmd/layerxctl`, `cmd/layerx-verify`, `cmd/layerx-genesis` |
| SDK / daemon | Call LayerX from an agent process | `agent/` — Rust workspace: types, canonical encoding, crypto, proofs, daemon, SDKs |
| MCP tools | Hand a scoped key to an agent | `agent/crates/layerx-mcp` — one tenant, one scope, daemon-only routing. CLI installer: `platform/cli/` (`layerx install mcp`) |
| Interop | Speak x402, AP2, A2A, and the rest at the edge | `interop/` — adapters translate; they do not write balances |
| Paxeer | Settlement and custody L1 | `paxeer-network/` — `paxd`, EVM/RPC, chain modules. LayerX settlement contracts stay in repo-root `contracts/` |

The agent workspace has no protocol authority. Every state change is a signed LayerX activity. The node interface is the only boundary into the C17 core. `402LXP` is the only balance writer.

---

## Build the node

From the repository root:

```
make build
make test
make test-contracts
```

Broader gates:

```
make public-audit
make ci
```

The core runtime is C17. LayerX settlement contracts are Solidity `0.8.27`. Agent, human, and platform workspaces are Rust.

---

## Build the agent interface

Agent crates build independently of the C core:

```
make agent-check-boundary
```

`make agent-check-boundary` rejects storage dependencies, node-private paths, and C-core linkage. The agent layer never invents protocol state.

Languages called out on the site: Rust, TypeScript, Python. Generated / language SDKs live under `agent/sdk` and `agent/tools/sdk-gen`.

MCP in this workspace: [`agent/crates/layerx-mcp`](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/agent/crates/layerx-mcp/README.md). Tools are evidence-shaped (`balance.get`, `receipt.get`, `activity.submit`, …). A read-only deployment omits write tools. Design note: [MCP tools](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/spec/layerx-agent-interface/docs/mcp-tools.md).

---

## MCP and A2A

Two complementary surfaces — both real, neither a second ledger.

| Surface | Role |
| --- | --- |
| Agent MCP server | `agent/crates/layerx-mcp` — bound at startup to one tenant and one scope; every call goes through `layerx-agentd` |
| Interop transports | `interop/` labels `http`, `mcp`, and `a2a`. x402 buyer / seller / facilitator run on all three. There is no standalone `layerx-a2a` crate |
| CLI installers | `layerx install mcp` and `layerx install a2a` in `platform/cli/`. Copy: [install](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/platform/docs/content/install.md), [interop guide](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/platform/docs/content/guide/interop.md) |

A2A is a transport and a loopback agent-card runtime, not a custody path.

---

## Ethereum and Solana mirrors

Batch mirror archives live in `interop/crates/layerx-mirror`. Publisher and verifier binaries: `layerx-mirror-publisher`, `layerx-mirror-verify`. On-chain archive programs: `interop/contracts/ethereum-mirror/`, `interop/contracts/solana-mirror/`.

Mirrors are presence, not custody. Anyone can verify a LayerX receipt from a mirror. Funds do not move to Ethereum or Solana. Settlement stays on Paxeer.

```
make interop-build
make interop-test
```

Live publisher / verify targets (`make mirror-live`, `make mirror-verify-live`) are operator qualification entrypoints.

---

## Migrations

Three different words, three directories:

| Kind | Location |
| --- | --- |
| LayerX genesis / cutover from the prior Go system | `spec/layerx-protocol/docs/migration.md`, SQL in `migrations/`, CLI in `cmd/layerx-genesis/` |
| Ethereum / Solana source-chain import | `interop/crates/layerx-migrate` — [operations](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/interop/crates/layerx-migrate/OPERATIONS.md) |
| Paxeer EVM store migrations | `paxeer-network/modules/evm/migrations/` (chain-internal) |

---

## Build Paxeer (in this repo)

Paxeer Network is checked in under `paxeer-network/`. It is the settlement L1, not a LayerX rewrite.

From the monorepo root:

```
make paxeer-build
make paxeer-lint
make paxeer-test
make paxeer-ci
```

From `paxeer-network/`:

```
make build
make test
forge install && forge build
```

Subtree README: [`paxeer-network/README.md`](https://github.com/Sidiora-Labs/LayerX-Protocol/blob/main/paxeer-network/README.md). Docs index: [`paxeer-network/docs/`](https://github.com/Sidiora-Labs/LayerX-Protocol/tree/main/paxeer-network/docs).

---

## What you can do now

- Inspect and replay the protocol from source
- Run a local node and verify batches (`layerx-verify`)
- Wire an agent to canonical encoding, signatures, and receipts
- Scope a key and attach MCP tools to that key
- Read the interop adapters, mirror publisher, and migration verifiers
- Build `paxd` from `paxeer-network/`

What opens September 7 is limited beta access to the public lane — not a rewrite of these binaries, and not a public LayerX RPC.

---

## Spec first

Normative behavior is KVX in `spec/`. Generated Markdown is easier to read; changes begin in KVX.

Payments: Payments · Fees. Security model: Security. Project stage: Status. Settlement: still Paxeer.
