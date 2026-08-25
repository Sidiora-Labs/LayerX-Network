# LayerX-Protocol Module Map

Generated from subagent exploration (agent/contracts/programs/interop/platform/human/src/cmd/tests/docs) + workspace scan.

---

## 1. src/ + include/ — C17 Protocol Kernel

- **Files:** ~30 `.c` files in subdirs (codec, crypto, guarantor, ledger, modules, network, paxeer, protocol, replica, sequencer, state, storage) + headers (`lxp_kernel.h`, `lxp_ledger.h`, `lxp_crypto.h`)
- **What it does:** Deterministic execution engine, cryptographic primitives, ledger/state modules (asset, bridge, budget, escrow, governance, perps, programs, service, stream), network/gateway, sequencer.

## 2. contracts/ — Solidity Contracts (Foundry)

- **Files:** `LayerXCustody.sol`, `CheckpointRegistry.sol`, `LayerXVault.sol`, `EmergencyExit.sol`, `WithdrawalClaims.sol`, `GuarantorBond.sol`, `LayerXTimelock.sol`, 17 library contracts, interfaces, security patterns.
- **What it does:** On-chain custody/checkpoint/bond/governance infrastructure for the protocol.

## 3. agent/ — Agent Boundary Layer (Rust)

- **Files:** `agent/crates/layerx-agentd/src/lib.rs` (daemon), `agent/crates/layerx-agent-api/src/lib.rs` (contract), SDK, client, types, wire, crypto, proof, mcp.
- **What it does:** Non-authoritative boundary to C core; enforces strict separation (no node DB access, no protocol authority); `make agent-check-boundary` enforces dependency rules.

## 4. programs/ — WASM Runtime + Registry (Rust)

- **Files:** `programs/crates/layerx-programs-runtime/src/lib.rs` (WASM engine, ABI), registry (`lib.rs`, deploy/upgrade/deprecate), fuzz targets.
- **What it does:** Deterministic WASM runtime for permissionless economic apps; capability-based kernel APIs; namespaced storage; metering.

## 5. interop/ — Interoperability Gateway (Rust)

- **Files:** `interop/crates/layerx-interop-gateway/src/gateway.rs` (adapter registry), adapter crates (`layerx-x402`, `layerx-ap2`, `layerx-ucp`, `layerx-visa-tap`, `layerx-fiat`), portable verification (`layerx-portable`), mirror/archive.
- **What it does:** Edge-only translation layer; adapters translate but never gain protocol authority; every external interaction terminates in receipt-verified LayerX operation.

## 6. platform/ — Developer Toolchain (Rust + TypeScript + docs)

- **Files:** `platform/cli/src/main.rs` (CLI), `platform/sdk/*` (multi-language SDKs), middleware (`buyer/`, `seller/`, `merchant/`, `agent/`), docs site, emulator, hosted services (`testnet/`, `gateway/`, `faucet/`), release pipeline.
- **What it does:** CLI for payments/programs; SDK parity suite; middleware for autonomous agents/merchants; hosted testnet/gateway/webhooks.

## 7. human/ — Human Control Plane (Next.js + Rust)

- **Files:** `human/apps/web/src/` (Next.js app), `packages/layerx-ui/` (component library), `human/crates/layerx-intents/src/lib.rs` (typed-intent compiler — sole payload authority), `layerx-human-service/`, `layerx-paxeer-client/`, `layerx-explorer-index/`.
- **What it does:** User-facing app reporting only verified receipts; exposes exactly 5 ideas (log in, add money, move money, manage agents, see what happened); typed-intent compiler produces canonical bytes for disclosure-bound signing.

## 8. cmd/ — Executable Entry Points (C)

- **Files:** `cmd/layerxd/lxp_daemon_main.c` (daemon), `cmd/layerx-genesis/lxp_genesis_main.c`, `cmd/layerx-verify/lxp_verify_main.c`, `cmd/layerxctl/lxp_ctl_main.c`.
- **What it does:** Executables for running the C kernel, initializing genesis, verifying batches, submitting activities.

## 9. tests/ — C Test Suite

- **Files:** `tests/protocol/`, `tests/crypto/`, `tests/ledger/`, `tests/state/`, `tests/modules/`, `tests/arith/`, `tests/codec/`, `tests/network/`, `tests/programs/`, `tests/qualification/`, `tests/replay/`, `tests/sequencer/`, harness (`lxp_test_harness.c`).
- **What it does:** Unit/integration/property/replay/fault-injection/replay qualification tests.

## 10. fuzz/ — Fuzz Targets

- **Files:** `fuzz/lxp_fuzz_codec.c`, `fuzz/lxp_fuzz_arith.c`, `fuzz/lxp_fuzz_batch_header.c`, `fuzz/lxp_fuzz_gateway_json.c`, `fuzz/lxp_fuzz_send.c`, `fuzz/lxp_fuzz_meter.c`, `fuzz/lxp_fuzz_signature.c`, `fuzz/lxp_fuzz_activity.c`, `fuzz/lxp_fuzz_transfer_set.c`, corpus (`corpus/`).
- **What it does:** Continuous fuzzing of serialization, arithmetic, signatures, gateway parsing.

## 11. spec/ — Specification Source + Generator

- **Files:** `spec/workflow.kvx`, `spec/specgen/main.go`, active feature specs (`spec/layerx-protocol/`, `spec/layerx-platform/`, `spec/layerx-agent-interface/`, `spec/layerx-human-interface/`), qualification logs (`qualification.kvx`).
- **What it does:** Normative `.kvx` specs that generate IDE rules (`AGENTS.md`, `.cursor/rules/`, `.claude/`) and feature docs; qualification logs record conflicts/ambiguities.

## 12. docs/ — Documentation + Qualification

- **Files:** `docs/QUALIFICATION.md`, design docs (generated from `spec/`), `CHANGELOG.md`, `README.md`, `SECURITY.md`.
- **What it does:** Qualification gates, design references, changelog.

## 13. migrations/ — SQL Migrations

- **Files:** `0001_genesis_sections.sql`, `0001_projection.sql`, `0007_history_index.sql`.
- **What it does:** Database schema evolution.

## 14. tools/ — Build/CI/Replay Support

- **Files:** `tools/ci/no-float-scan.sh`, `symbol-allowlist.sh`, replay fixtures (`lxp_replay_fixture.py`, `lxp_replay_matrix.sh`), proof scripts (`proofs/`), workspace gate scripts.
- **What it does:** Qualification/replay/build verification infrastructure.

## 15. layerx (shell script) — Build Entry Wrapper

- **What it does:** Main entry script (recently run with exit 0, no output — likely a build/test wrapper).

## 16. Makefile — Main Build Orchestration

- **What it does:** C compilation (`build/liblayerx.a`), Rust workspace builds (`agent/`, `programs/`, `interop/`, `platform/`), test targets (`make test`), fuzz targets, qualification/replay targets.

---

## Quick Navigation (by purpose)

| Purpose | Directory |
|---|---|
| Protocol core | `src/` + `include/` |
| Smart contracts | `contracts/` |
| Agent boundary | `agent/` |
| Programs runtime | `programs/` |
| Interop gateway | `interop/` |
| Dev toolchain | `platform/` |
| Human interface | `human/` |
| Executables | `cmd/` |
| Tests | `tests/` |
| Fuzz | `fuzz/` |
| Specs (SSOT) | `spec/` |
| Migrations | `migrations/` |
| Build support | `tools/`, `Makefile`, `layerx` |

---

## Subagent Results (stored IDs)

- agent: `0da8e38b`
- contracts: `9a95f933`
- programs: `bb8d2fe6`
- interop: `44269a2d`
- platform: `2a99ee95`
- human: `175732fb`
- src/core: `d4182bbf`
- cmd/tests: `d0a39c21`
- docs/spec: `39b801ef`
