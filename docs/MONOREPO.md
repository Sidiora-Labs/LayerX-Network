# Monorepo layout

This repository is the canonical Sidiora Labs ecosystem monorepo for LayerX and the Paxeer Network. Co-location keeps the protocol, settlement network, contracts, developer surfaces, and their automation auditable in one place while preserving their separate build, release, deployment, and trust boundaries.

## What lives where

| Path | Subsystem | Build entry | Release tags |
| --- | --- | --- | --- |
| `src/`, `include/`, `agent/`, `human/`, `platform/`, `programs/`, `interop/`, `contracts/`, `spec/`, `tests/`, `fuzz/`, `migrations/` | LayerX protocol, agent interface, human control plane, developer platform, programmable runtime, interoperability gateway, and settlement contracts | Root `Makefile` | `vX.Y.Z` |
| `paxeer-network/` | Paxeer Network node, EVM/RPC compatibility, storage engines, modules, contracts, Docker environments, and subsystem-local build manifests | `paxeer-network/Makefile` | `paxeer-network/vX.Y.Z` |

## Build and release boundaries

LayerX and Paxeer have independent build systems, release processes, and qualification gates:

- **LayerX**: C17, Rust, Solidity, TypeScript. Built with the root `Makefile`. Qualified with `make ci`, replay, arithmetic, fault, and fuzz suites.
- **Paxeer**: Go, Solidity, Rust, Docker. Built with `make paxeer-build`, `make paxeer-lint`, `make paxeer-test`, `make paxeer-ci`.
- **Monorepo integrity**: `make monorepo-ci` runs cross-subsystem checks but does not replace either subsystem's own qualification.

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

File paths under `paxeer-network/` trigger Paxeer-specific CI. Changes outside that directory do not run Paxeer builds unless explicitly configured.

## Further reading

- Root `README.md`: Ecosystem overview and repository layout
- `CONTRIBUTING.md`: Contribution guidelines for both subsystems
- `docs/QUALIFICATION.md`: Evidence levels and qualification gates
