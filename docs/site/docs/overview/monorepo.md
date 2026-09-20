# Monorepo layout

This repository is the canonical Sidiora Labs ecosystem monorepo for LayerX Network and the Paxeer Network. Co-location keeps the protocol, settlement network, contracts, developer surfaces, and their automation auditable in one place while preserving their separate build, release, deployment, and trust boundaries.

## What lives where

| Path | Subsystem | Build entry | Release tags |
| --- | --- | --- | --- |
| `src/`, `include/`, `agent/`, `human/`, `platform/`, `programs/`, `interop/`, `contracts/`, `spec/`, `tests/`, `fuzz/`, `migrations/` | LayerX protocol, agent interface, human control plane, developer platform, programmable runtime, interoperability gateway, and settlement contracts | Root `Makefile` | `vX.Y.Z` |
| `go.mod`, `chain.mk`, `daemon/`, `node/`, `modules/`, `consensus/`, `sdk/`, `rpc/`, `precompiles/`, `storage/`, `wasm/`, `docker/` | Paxeer Network node, EVM/RPC compatibility, storage engines, modules, contracts, Docker environments, and subsystem-local build manifests | `Makefile` | `paxeer-network/vX.Y.Z` |

## Build and release boundaries

LayerX and Paxeer have independent build systems, release processes, and qualification gates:

- **LayerX**: C17 (`-std=c17` in the root `Makefile`), Rust 1.91.1 (`rust-toolchain.toml`), Solidity 0.8.27 (`foundry.toml`), TypeScript. Built with the root `Makefile`: `make build`, `make test`, `make test-contracts`, `make ci`. Qualified with `make ci`, `make qualify-replay`, `make qualify-arith`, `make qualify-faults`, and `make qualify-fuzz`. Replay qualification needs GCC 13, Clang 18, Docker, an amd64 musl runner, and an AArch64 cross-compiler plus QEMU; see `docs/QUALIFICATION.md`.
- **Paxeer**: Go, Solidity, Rust, Docker. Built with `make paxeer-build`, `make paxeer-lint`, `make paxeer-test`, `make paxeer-ci`.
- **Monorepo integrity**: `make monorepo-ci` runs cross-subsystem checks but does not replace either subsystem's own qualification. `make ci` runs `public-audit`, native tests, a two-build archive comparison, consensus symbol checks, and sanitizer suites.

Release tags follow the pattern:
- LayerX: `vX.Y.Z`
- Paxeer: `paxeer-network/vX.Y.Z`

## Trust boundaries

Co-location in this repository does not grant one subsystem new authority over the other:

- LayerX protocol execution, balance writes, and activity ordering remain under LayerX's deterministic runtime and specification.
- Paxeer custody, checkpoint registration, guarantor bonds, challenges, and emergency exits remain under Paxeer's settlement contracts and chain.
- Shared source control does not imply shared deployment authority, validator sets, or custody semantics.

## Workflow naming

GitHub workflows and CI jobs use prefixed names to make subsystem ownership clear:

- LayerX workflows: `agent.yml`, `human.yml`, `platform.yml`, `programs-conformance.yml`
- Paxeer workflows: `Paxeer / Build`, `Paxeer / Lint`, `Paxeer / Test`

The Paxeer chain sources sit at the repository root next to the LayerX trees, so no single directory scopes Paxeer CI. Each `paxeer-*.yml` workflow lists the chain paths it covers explicitly: the root Go module files (`go.mod`, `go.sum`, `chain.mk`, `foundry.paxeer.toml`, `Dockerfile`), the chain-only directories (`daemon/`, `node/`, `modules/`, `consensus/`, `sdk/`, `rpc/`, `precompiles/`, `storage/`, `wasm/`, and the rest of that list), and the chain-owned subpaths of the shared directories (`contracts/src/`, `contracts/test/`, `docs/swagger/`, `scripts/`, `tests/chain/`, `tools/chain/`, `tools/tx-scanner/`, `tools/utils/`). Changes outside those paths do not run Paxeer builds.

## Further reading

- Root `README.md`: Ecosystem overview and repository layout
- `CONTRIBUTING.md`: Contribution guidelines for both subsystems
- `docs/QUALIFICATION.md`: Evidence levels and qualification gates
