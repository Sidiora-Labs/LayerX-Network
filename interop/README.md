# LayerX interoperability workspace

Adapters at the edge. They translate someone else's protocol into LayerX-shaped evidence, and they never write balances. `402LXP` remains the only balance writer. Custody and withdrawal guarantees stay on Paxeer.

This is the Rust workspace in `interop/`. It is not a second ledger.

## Where to start

| Surface | What it is | Where it lives |
| --- | --- | --- |
| Gateway | Transport-neutral routes, redaction, adapter host | `crates/layerx-interop-gateway`, executable composition in `crates/layerx-interop-service` |
| MCP / A2A transports | Ingress labels `mcp` and `a2a` next to `http` | Gateway `IngressTransport`; x402 `TransportKind` |
| x402 v2 | Buyer, seller, facilitator over HTTP, MCP, and A2A | `crates/layerx-x402` — [COMPATIBILITY.md](crates/layerx-x402/COMPATIBILITY.md) |
| Ethereum / Solana mirrors | Batch archive publication and verification. Pure archives: no vault, no portal, no custody | `crates/layerx-mirror`; contracts in `contracts/ethereum-mirror/` and `contracts/solana-mirror/`; deploy notes in `deploy/mirror/` |
| Ethereum / Solana migration | Source-chain verifiers and the `migration` adapter | `crates/layerx-migrate` — [OPERATIONS.md](crates/layerx-migrate/OPERATIONS.md) |
| Portable receipts | Verify LayerX receipts without a live node | `crates/layerx-portable` — [PORTABILITY.md](PORTABILITY.md) |
| Other adapters | AP2, UCP, Visa TAP, fiat rails | `crates/layerx-ap2`, `crates/layerx-ucp`, `crates/layerx-visa-tap`, `crates/layerx-fiat` |

There is no standalone `layerx-a2a` crate. A2A is a transport on the gateway and on x402, plus the CLI installer in `platform/cli/`.

## Two MCP surfaces

| Surface | Role |
| --- | --- |
| `agent/crates/layerx-mcp` | Tenant- and scope-bound MCP server that routes every call through `layerx-agentd` |
| This workspace + `platform/cli/` | MCP as an interop ingress; `layerx install mcp` / `layerx mcp serve` from the developer CLI |

Normative MCP tool design: [`spec/layerx-agent-interface/docs/mcp-tools.md`](../spec/layerx-agent-interface/docs/mcp-tools.md). CLI install copy: [`platform/docs/content/install.md`](../platform/docs/content/install.md) and [`platform/docs/content/guide/interop.md`](../platform/docs/content/guide/interop.md).

Interop service deployment inputs, including the required protocol network,
authoritative module registry, server-owned TAP clock skew, explicit trusted
agent status, and per-principal canonical TAP targets, are documented in
[`deploy/gateway/`](deploy/gateway/README.md).

## Mirrors are archives

`layerx-mirror-publisher` and `layerx-mirror-verify` publish and check batch commitments on Ethereum and Solana. Anyone can verify LayerX state from a mirror. Funds do not live on those chains. Settlement stays on Paxeer (EVM chain ID `125`), whose node now lives in [`paxeer-network/`](../paxeer-network/).

Remote signer framing: [`deploy/mirror/signer-protocol.md`](deploy/mirror/signer-protocol.md).

## Migrations (three different things)

| Kind | Location |
| --- | --- |
| ETH / Solana source migration into LayerX | this workspace, `crates/layerx-migrate` |
| LayerX genesis / cutover from the prior Go system | [`spec/layerx-protocol/docs/migration.md`](../spec/layerx-protocol/docs/migration.md) and [`migrations/`](../migrations/) |
| Paxeer EVM store migrations | `paxeer-network/modules/evm/migrations/` (chain-internal; not a LayerX surface) |

## Build and test

From the monorepo root:

```sh
make interop-build
make interop-test
make interop-lint
make interop-test-x402
make interop-test-migration
```

Mirror publisher / verifier binaries:

```sh
cargo build --locked --release --manifest-path interop/Cargo.toml --package layerx-mirror --bin layerx-mirror-publisher
cargo build --locked --release --manifest-path interop/Cargo.toml --package layerx-mirror --bin layerx-mirror-verify
```

`make mirror-live` and `make mirror-verify-live` are qualification entrypoints; they require operator configuration and are not a public RPC.
