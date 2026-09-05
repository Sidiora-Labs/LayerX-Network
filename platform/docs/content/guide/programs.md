# Programs

A LayerX program is deterministic WASM that runs against protocol state. It can compute, use principal-scoped and shared storage, call other programs, and request transfers through explicitly granted authority. It never writes a balance.

## The monetary law

This is the single most important thing about programs, and it is structural rather than advisory.

Guest code never mutates a balance. What guest code can produce is a typed 402LXP transfer request, and it can only produce one through a `TransferCapability` it was actually granted. The capability authorises the effect set; the kernel transfer primitive applies it.

## Program-owned value

A program may hold value in an ordinary protocol account derived from its
program identifier and a public seed. Anyone can reproduce the account
identifier; nobody can turn that identifier into authority. Funding is an
ordinary principal-funded 402LXP transfer to the derived account. Spending is
a separate `ProgramSpend` grant binding the owner program, seed, rederived
source, asset, destination and cumulative amount ceiling. Only the deriving
program's own frame can stage that debit, and the kernel transfer primitive is
still the only balance writer.

The Rust SDK examples are complete custody patterns:

- `programs/sdk/rust/examples/escrow` records distinct immutable release and
  refund receipt digests in shared state, takes payment into its derived
  account, and pays exactly once after the selected receipt verifies a
  successful settlement for the escrow's exact asset and amount.
- `programs/sdk/rust/examples/vault` credits each caller in principal-scoped
  storage while maintaining the pooled total in shared storage. A withdrawal
  debits both ledgers and the real derived account in one atomic execution.

A program cannot derive another program's authority, stage a derived-account
debit from a callee frame, write balances, mint or burn value, perform an
unbounded whole-balance sweep, or treat bookkeeping storage as money. EVM
contract balances, Solana PDA-held value and CosmWasm contract balances map to
the same derived-account pattern. Allowances over third-party funds, direct
lamport writes, supply-burning messages and cross-chain transfers remain named
refusals where their source semantics are not representable.

```
guest program  ->  AbiEffects
TransferCapability::authorize  ->  AtomicTransferSet
KernelTransferPrimitive::apply_and_verify_402lxp_set  ->  VerifiedProgramSettlement
```

The kernel owns all balance mutation, conservation enforcement, atomic rollback, receipt emission and receipt verification. `VerifiedProgramSettlement` has no successful constructor that bypasses the verifier: there is no way to produce one except by the kernel having actually applied and verified the set.

The refusal taxonomy is closed and each variant means one thing:

| Refusal | Cause |
|---|---|
| `UnverifiedAuthority` | The invocation authority was not verified |
| `InvalidTransfer` / `InvalidTransferSet` | The request or the set is malformed |
| `AmountOverflow` | The set's total exceeded the amount range |
| `InvariantViolation` | A monetary bypass was detected - the guest tried to move value outside the law |
| `CapabilityEscalation` | A child call's transfer exceeded the narrowed authority it was called with |
| `KernelRefused` | The kernel refused the set |
| `ReceiptInvalid` / `ReceiptMismatch` | The settlement receipt is invalid, or does not bind this exact set |

`CapabilityEscalation` is what makes composition safe. Calling another program hands it a narrowed authority, and a callee that tries to spend beyond it is refused rather than trusted.

## Determinism

Determinism is enforced at build time and at execution time, not requested in a style guide. The validator rejects modules that reach for non-determinism; the meter bounds fuel, memory and storage against a declared `ResourceBudget` and a `FeeSchedule`; composition is bounded by maximum depth, fan-out, call-graph edges and program visits. A program that exceeds any declared limit is refused - it does not run halfway and leave state behind.

Replay is first-class: a recorded execution can be replayed and any divergence reported as a `ReplayRefusal`. The same inputs produce the same outputs on every node, which is what lets a receipt mean anything.

## Building and deploying

ABI 2 is the current frozen guest ABI. ABI 1 remains supported for legacy programs;
ProgramSpend and BalanceView grants require ABI 2.

**Funding prerequisite:** Ordinary genesis starts with zero balances. Program calls require an authenticated custody-funded actor with the required fee asset. Follow the [custody profile and real-deposit workflow](../../../../docs/wiki/Custody.md#reproducible-local-funding), using the genesis/fee asset, and verify the funding receipt before spending. A custody profile does not preallocate balances; do not substitute prefunding or state injection. Local funding qualification does not establish production finality.

**Escrow account prerequisite:** After deployment and before `OPEN`, register the escrow's derived value account through a real Programs account-registration activity (module 9, ordinal 6). Submit the canonical signed protocol-3 activity to authenticated `POST /v1/activities` as `application/octet-stream`, with its `Idempotency-Key`, and verify the receipt before calling `OPEN`. The payload is `program_id(32) ‖ "LXPA1"(5) ‖ asset_id(32) ‖ seed_length(u32 big-endian) ‖ seed`: exactly `73 + seed_length` bytes, with seed length at most 128. Use the same program, asset and seed as the escrow call, signed by an authorized registration principal. Deployment does not automatically register this account; there is no dedicated registration CLI command.

```
layerx program build --manifest-path ./Cargo.toml
layerx program deploy ./target/program.wasm --program-id <hex32> --upgrade-authority <hex32> --interface ./interface.bin --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program upgrade ./target/program.wasm --program-id <hex32> --old-hash <hex32> --migration-hook ./migration.bin --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program wind-down route --program-id <hex32> --account <hex32> --asset <hex32> --destination <hex32> --seed <hex> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program wind-down deprecate --program-id <hex32> --exit-program <hex32> --deadline-batch <batch> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program wind-down tombstone --program-id <hex32> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program wind-down exit --program-id <hex32> --account <hex32> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end> --previous-state-root <verified-hex32>
layerx program call <hex32> --entrypoint layerx_call --abi-version 2 --calldata <hex> --fuel <units> --fee-limit <units> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end>
layerx program simulate <hex32> --entrypoint layerx_call --abi-version 2 --calldata <hex> --fuel <units> --fee-limit <units> --idempotency-key <hex32> --account-sequence <n> --not-before-ms <start> --expires-at-ms <end>
layerx program registry get <program-id>
```

`build` compiles to WASM and enforces the deterministic runtime policy locally, before anything is submitted - so a policy violation is a local failure, not a rejected deployment.

`deploy`, `upgrade`, `wind-down`, `call`, and `simulate` construct and sign canonical protocol-3 activities locally, then send their exact bytes as `application/octet-stream`. Select the local signing key with `--key`, or use the configured default. The environment must have a pinned sequencer trust anchor. Idempotency keys and program identifiers are 32-byte hex values. Use a distinct key and the current account sequence for each mutation; validity intervals must be nonempty and at most 300000 milliseconds.

Deployment and upgrade read `abi_version` (or the existing `abi` spelling) from the nearest enclosing `LayerX.toml` and `layerx-program.json`, defaulting to ABI 2. Conflicting declarations are refused. Omitting `--upgrade-authority` makes deployment immutable. The program identifier is chosen by the caller and signed into the payload, not invented by the server.

`--interface` takes canonical binary interface encoding, **not** `interface.kvx` source. Its digest-bound code hash and ABI must match the artifact. Upgrade preserves the existing interface unless supplied a replacement or `--clear-interface`; clearing and replacing are mutually exclusive. The optional migration-hook file contains the exact hook bytes. Route seeds are hex, at most 128 bytes; the default is empty.

Lifecycle verification requires `--previous-state-root` from independently verified committed state. The CLI checks the canonical receipt signature, protocol/module version, prior root, and exact signed activity ID. Lifecycle receipts carry state operation 0; the activity ID binds the deploy, upgrade, or wind-down ordinal. Refusals remain refusals. An unknown submission result retains signed bytes for reconciliation, rather than claiming completion. Calls additionally verify terminal and call-graph commitments against receipt evidence.

Call and simulation verification require an authenticated current program head; unsigned registry metadata is not a substitute. When a call acknowledgement contains only its activity ID and receipt, the CLI retrieves execution material from `/v1/programs/activities/{activity_id}`, binds it to that same receipt, and then verifies the terminal commitments. Program GET requests carry the bounded identity and verification-level selector required by the emulator.

Calls accept repeated `--capability` grants using the native capability set:

- `storage-read`, `storage-write`, `shared-storage-read`, `shared-storage-write`, `emit-event`
- `call:<program-hex32>`
- `transfer402:<asset-hex32>:<to-hex32>:<maximum-u128>`
- `receipt-read:<receipt-digest-hex32>`
- `program-spend:<owner-program-hex32>:<seed-hex>:<source-account-hex32>:<asset-hex32>:<to-hex32>:<maximum-u128>`
- `balance-view:<account-hex32>:<asset-hex32>:<receipt-digest-hex32>`

Unscoped legacy `transfer` and `compose` grants are refused. `--access-declaration` accepts canonical declaration hex; omission encodes an explicit absence marker, not an empty access set. Resource ceilings are configurable with `--memory-bytes`, `--storage-read-bytes`, `--storage-write-bytes`, `--output-values`, `--output-bytes`, `--table-elements`, and `--response-capacity`. Registry listing is not a production route and has no CLI command.

## Interpret or compile

The deterministic interpreter is an authoring convenience, not a cheaper or
equivalent execution tier. It is an ordinary ABI-v2 program: the outer Wasm
engine meters its instruction fuel, memory, storage reads, storage writes,
output values, output bytes and persistent occupancy through the same
protocol-owned `ResourceBudget` and `FeeSchedule` used for compiled programs.
The script decoder, bounds checks, register operations and control-flow loop
are therefore real metered work in addition to the host operations the script
requests.

The published release ceilings for the representative v1 arithmetic, storage,
transfer and bounded-control workload set are **12.00x compiled protocol fee**
and, separately, **12.00x compiled execution time**, each with a **15%
regression tolerance** (hard gates at 13.80x). These are declared release
thresholds, not observed results. Protocol fee is the economic comparison an
agent uses; wall-clock time is an operator performance signal and is never
presented as protocol price. `make programs-bench` builds the
real interpreter and its real compiled ABI-v2 equivalents, executes both
through the production ABI-v2 executor, reports median integer nanoseconds
and every metered resource and fee class, and refuses the release if either
aggregate ratio exceeds its gate. Human qualification records observed results
and the fixed hardware and software conditions; this guide does not invent
them. The broader cold/warm execution baseline and performance ledger remain
the qualification-owned task 32.7 component of this aggregate Make entry.

Use interpretation when removing a compiler from an agent's deployment path
is worth a potentially material execution premium: small policies, bounded
automation, infrequent jobs, or logic expected to change before its execution
cost dominates. Compile repeated, compute-heavy, latency-sensitive or
high-volume logic. An agent can make that choice mechanically: estimate the
expected invocation count, multiply the compiled protocol-fee estimate by 12 for
admission planning, and compile when that conservative lifetime premium costs
more than operating the toolchain. Both routes have identical authority and
isolation rules; changing routes cannot grant capabilities.

The benchmark additionally refuses any workload whose committed storage,
effects, receipt identity, runtime/schedule versions, call graph or receipt
outcome differs between the interpreted and compiled routes. Only metered
usage and the fee derived from it may differ. This equivalence check uses real
runtime types and the fail-closed receipt oracle; it is not a mock guest.

## The registry

The registry is the record of what is deployed, and it is receipt-backed rather than self-asserted. Source verification binds a program's code hash to a build environment - builder image digest, toolchain digest, dependency lock digest, `SOURCE_DATE_EPOCH`, and the exact command - so a third party can reproduce the artifact and check the binding themselves. A registry record with unverified source says so.

Deprecation is a wind-down, not a switch: a deprecated program still lets value accounts exit. The registry models that explicitly through the deprecation and wind-down views rather than leaving stranded balances to a migration script.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Programs never write balances | `protocol` | Guest code produces typed transfer requests; the kernel owns every balance mutation. |
| Typed program failure with rollback | `protocol` | A refused or faulted execution leaves no partial state. |
| Deterministic program execution | `protocol` | Validation, metering and composition bounds are enforced at build and at execution. |
| Conserved supply | `protocol` | Conservation is checked by the kernel primitive, not by the program. |
| Atomic settlement | `protocol` | The transfer set applies whole or not at all. |
| Offline receipt verification | `protocol` | The settlement result is bound to a verified receipt digest. |
