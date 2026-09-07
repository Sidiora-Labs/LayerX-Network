# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

#### Protocol runtime
- Protocol 3 occupancy wire for Programs calls, with explicit protocol-version assertions in the foundation crates
- Canonical execution step commitments and incremental sandbox usage settlement
- Persisted canonical finality evidence with adversarial recovery coverage
- Unified checkpoint identity and freshness across C, Rust, and Solidity
- Batch-header protocol versions preserved across proof planes
- Canonical log durability boundaries
- Fail-closed Programs monetary gate through canonical entrypoints
- Signed metering genesis for lifecycle migration tests
- Fee replay meters linked into the Programs runtime

#### Agent SDKs and agentd
- Agentd budgets, approvals, protocol evidence, and durable approval-decision persistence
- MCP and A2A installation surfaces, including authenticated durable A2A execution
- Verified Programs transports and receipt attachment checks in TypeScript, Python, Go, Rust, JVM, Swift, and .NET
- Applied terminal-transfer verification across those SDKs, with signed terminal vectors
- ABI v2 SDK bindings, porting bindings, and catalog generation
- JVM SDK lookup idempotency and Maven Central release metadata checks
- Privileged human component boundary on the agent plane
- Agent tokens bound to a durable revocation generation
- Encoder-derived simulation envelope goldens for the client

#### Hosted platform
- Hosted gateway, core, authority, registry, identity, internal, webhooks, faucet, and testnet control services
- Interoperability gateway with grouped store requests
- Middleware packages for agent, buyer, merchant, seller, and conformance
- Reference applications, mobile and agent integrations, and the market-maker ramp kit
- Developer dashboard and hosted webhooks with scoped receipt credentials
- Beta cluster bring-up from this repository, including an independent Paxeer observer and published TLS origins
- Emulator Programs hosts, resource errors, and snapshot persistence for committed Programs blobs
- CLI credential handling through a private Secret Service
- Release manifest emission with published-byte verification before promotion
- Sealed hosted-registry deployment envelopes

#### Programs
- Kernel Programs module occupancy settlement and protocol-backed program balances
- Program-owned account registry, transfer authority, and spending capability
- Deterministic interpreter, compiled-module cache by versioned code hash, and a single host linker
- Protocol-owned metering, fee schedules in protocol state, and access-set binding
- Parallel scheduling of non-conflicting program activities
- Sandbox leases as protocol state, with lease escrow, capability isolation, expiry, and snapshot metering
- Compute marketplace program with attested inputs and challenge-window usage settlement
- Frozen Programs ABI version two, with strict metadata parse, immutable linker closure, and lint routing
- Program custody reference patterns isolated from host builds
- Receipt-verified program balance sight and authenticated execution context
- Portable receipt verifier and program porting crates for CosmWasm, EVM, and Solana
- Storage-scan host contract and typed rollback behavior
- Cross-SDK Programs receipt verification and explorer state refresh

#### Human
- Production Human agent boundary with authenticated durable admission
- Structured API errors in the envelope and typed explorer overload state
- Evidence verifier required before any receipt-verified Human label
- Protocol-specific deployment helper in Human withdrawal tests

#### Contracts
- Custody, vault, checkpoint, guarantor-bond, timelock, withdrawal, and emergency-exit contracts
- Ethereum and Solana mirror contracts, durable mirror publisher, and mirror verification surfaces
- Ethereum and Solana migration tooling
- Paxeer beta deployment validator and fixed-address USDL deployment constraints

#### Paxeer
- Paxeer settlement boundary with CometBFT genesis served through the boundary
- Disposable custody identity bound to genesis, with signed custody deployment through pinned TLS origins
- Custody credits produced from verified cluster RPCs
- Autobahn certificate, handshake, producer-arithmetic, and payload-construction checks
- Independent observer genesis identity across disposable Paxeer boundaries

#### Docs
- Wiki pages for Programs, sandbox, storage scan, SDK terminal verification, portable verifier, x402, Agentd, CLI, hosted services, beta cluster, and the Paxeer boundary
- Testnet quickstart covering cluster export and program deploy
- Agent runtime guide
- Paxeer docs site

#### Tooling
- Native `layerxd`, `layerx-genesis-build`, and `layerxctl` binaries and their test wiring
- Beta qualification driver and focused gate runner
- Android sample-app Gradle wrapper with dependency verification
- Vendored Programs workspace dependencies and wasmvm libraries
- x402 transport conformance matrix

### Changed

#### License
- Relicensed first-party LayerX code under the Apache License, Version 2.0
- Set first-party Cargo.toml, Solidity SPDX, and package.json license identifiers to Apache-2.0
- Added NOTICE with attribution for vendored components whose licenses require it

#### Protocol runtime
- Occupancy semantics for protocol-3 Programs calls through the runtime FFI
- Blake3 verification against official test vectors
- secp256k1 digest verification in the Programs runtime
- Interpreter relocations compressed for root Cargo builds
- Node toolchain initialized before parallel native builds

#### Agent SDKs and agentd
- SDK transport clocks and a real authority harness
- Generated API digest and platform SDK projections after envelope schema changes
- Public `Amount` type and canonical resource errors in the emulator

#### Hosted platform
- Hosted topology aligned with journey-specific testnet readiness
- Gateway, webhook, ramp, and dashboard handlers grouped onto shared request contracts
- Withdrawal settlement bound to the recorded asset
- Registry deployments published as one sealed envelope
- `getrandom` 0.4.3 on the platform workspace; base64 unified on 0.23.1
- Cluster lifecycle goals serialized under parallel make
- Source-bound artifact manifests for promotion

#### Programs
- ABI v2 as the crate-root current ABI, with historical constant paths restored
- Sandbox principal capacity released at lease expiry
- Canonical sandbox state reconstruction metered during restore
- Programs event and capability bounds aligned
- Guest marketplace built for its Wasm target

#### Paxeer
- Immediate Paxeer beta bootstrap with verified governance checks
- Deterministic Paxeer deployment topology
- Bounded Giga configuration defaults
- Paxeer finality endpoint pinned in the node egress contract

#### Docs
- Quickstart reconciled with the cluster export and program deploy walk
- Programs wiki citations and payload layouts corrected after fact-check
- Beta cluster page refreshed for the genesis route and observer identity wiring

#### Tooling
- cargo-deny invoked without the removed `--disable-fetch` flag
- CI on hosted runners; GHCR as the integration-test image source
- Fuzz lockfiles reconciled with current runtime dependencies
- Binaryen metering corpus made reproducible

### Fixed

#### Protocol runtime
- Typed state and snapshot validation errors preserved
- Aggregate event exhaustion treated as terminal
- Trapped call output reservations rolled back
- Programs receipt and state-proof parity restored
- Composition limits isolated from the event byte budget
- Storage-scan cursor, page, and typed rollback expectations corrected
- Signature expectations aligned with independently checked authority vectors
- C test link order and kernel header completeness

#### Agent SDKs and agentd
- Agentd, client, MCP, and SDK crates repaired for strict all-target Clippy
- MCP error contracts and borrows
- CLI seed, request handling, and ramp canonical length encoding
- Boundary conformance runner compiled against activity error types
- Interop crates (gateway, x402, visa-tap, UCP, portable, fiat, AP2, migrate, mirror) repaired for strict Clippy

#### Hosted platform
- Clean-profile emulator bootstrap
- Durable-storage faults distinguished from admin refusals in the core contract
- `batch_evidence` admitted on maintained authority responses
- Faucet, SDK generator, hosted registry, webhook, gateway, dashboard, testnet, and ramp lint findings
- Registry deployment contract builder-digest expectation
- Dashboard URLPattern globals for Node typings
- Environment files and untracked artifacts excluded from Docker contexts

#### Programs
- Interpreter, market, CosmWasm guest, independent runtime, and Rust program SDK Clippy findings
- Vendored wasmi, parity-wasm, and wasm-instrument Clippy findings with explicit elided lifetimes
- Programs receipt verifier widths, call-length arithmetic, and aggregate build blockers
- Native account and journal tests linked with OpenSSL
- Vendored Binaryen LLVM configuration header restored so programs-test builds
- Interpreter benchmark compiled against the validated module contract

#### Human
- API generator documentation; schema drift check
- Human activity evidence path before receipt-verified labels

#### Contracts
- Contract safety qualification harnesses
- ABI v2 selection, parity, and reference qualification
- Reference artifacts closed on read failures

#### Paxeer
- Autobahn fixtures signed with committee-bound keys; EVM messages rejected when tx data fails to unpack
- golangci findings: integer conversions, mismatched-package imports, KV cache sizing, receipt action strings
- Incomplete RPC execution data, unrepresentable Autobahn heights, and invalid consensus index ranges rejected
- Paxeer SDK compatibility panics removed
- secp256k1 signatures verified against the supplied digest

#### Tooling
- Platform CLI and ramp request parsing
- Workspace Rust ownership and helper errors that blocked builds
- Swift `Package.resolved` dropped from the beta driver test tree

### Removed

- Internal root files `CODEBASE_AUDIT_2026-08-28_REFRESH.md`, `METADATA_UPDATES.md`, `org-profile-update.patch`, and `module-map.md`
- `docs/wiki/Status.md` and `docs/wiki-drafts/`
- `LICENSE_NOTICE.md` after the Apache 2.0 relicense
- Fabricated Paxeer genesis commitments
- Unused base64 0.22.1 entry from the verify-receipt sample lock
- Duplicate interop `r-efi` crate
- CLI tests' rejected mock override, replaced by a private Secret Service

### Security

- Relicensed the first-party tree under Apache License 2.0 with an explicit patent grant
- Authenticate Autobahn state before waiting on peer progress
- Reject unauthenticated or incomplete Paxeer RPC execution data
- Bind disposable custody identity to genesis and preserve host refusals
- Serve verified receipt authority facts over TLS from an independent replica
- Authenticate and durably admit LNI activities
- Authenticate and isolate hosted program builds
- Harden Paxeer precompile boundaries and Autobahn peer handshakes
- Fail-closed release qualification runners and Programs monetary checks
- Webhook trust refusals and scoped receipt credentials
- Exact sandbox capability refusals required through valid guest calls
- Exact portable receipt refusals from the vector contract

[Unreleased]: https://github.com/Sidiora-Labs/LayerX-Protocol/commits/main
