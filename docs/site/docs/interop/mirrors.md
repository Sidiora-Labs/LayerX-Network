# Batch mirrors

Batch mirror archives are published to external chains as pure archives:
commitments plus retrievable data. No vaults, no portals, no custody
semantics (`spec/layerx-platform/spec.kvx`, decision `multichain_surface`
and requirement 34; `interop/crates/layerx-mirror/README.md`).

Anyone can verify LayerX state from a mirror. Funds do not live on those
chains. Settlement stays on Paxeer (EVM chain ID `125`).

## Binaries

| Binary | Role |
| --- | --- |
| `layerx-mirror-publisher` | `cargo run --bin layerx-mirror-publisher -- <config.json>` |
| `layerx-mirror-verify` | `cargo run --bin layerx-mirror-verify -- <config.json>` |

On-chain programs are `interop/contracts/ethereum-mirror/` and
`interop/contracts/solana-mirror/`. Remote signer framing is
`interop/deploy/mirror/signer-protocol.md`.

From the monorepo root: `make interop-build`, `make interop-test`.
Operator live targets `make mirror-live` and `make mirror-verify-live`
require operator configuration and are not a public RPC
(`interop/README.md`).

## Freshness

Requirement 34 `ac_3` requires each mirror to state its own freshness —
the latest batch and checkpoint mirrored — wherever mirror-derived data is
displayed. A lagging mirror must say so rather than presenting partial data
as current.

## Status

The publisher, verifier, Ethereum and Solana contracts, and tests are in
this tree. The platform specification also requires the explorer verifier
and SDK paths to verify receipts from a mirror with LayerX infrastructure
unavailable. That qualification gate is not claimed here.
