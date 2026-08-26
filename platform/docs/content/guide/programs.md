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

```
layerx program build --manifest-path ./Cargo.toml
layerx program deploy ./target/program.wasm --idempotency-key <key> --upgrade-authority <id> --source-uri <uri>
layerx program registry get <program-id>
layerx program registry verify-source <program-id> --source-uri <uri> --source-digest <hex> --idempotency-key <key>
```

`build` compiles to WASM and enforces the deterministic runtime policy locally, before anything is submitted - so a policy violation is a local failure, not a rejected deployment.

`deploy` validates the artifact and submits it for receipt-backed deployment. It takes an idempotency key, because deploying is a money-adjacent state change and deploying twice by accident is not acceptable.

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
