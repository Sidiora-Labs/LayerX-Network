# LayerX Programs

**The programmable runtime for LayerX — deterministic guest execution with no balance-writing authority.**

This workspace holds the LayerX programs surface: a deterministic WASM runtime, the
registry that proves what a program deployed and what it holds, the C↔Rust bridge into
the protocol kernel, developer SDKs, porting kits, and the adversarial test corpus.

Programs are a **first-class execution surface**, not one of the protocol's economic
modules. The eight economic modules (`0x01`–`0x08`) are documented in
[`src/modules`](../src/modules) and the project wiki; programs sit alongside them as the
place where untrusted guest code runs. Guest execution is dispatched under the programs
module inside the runtime module registry (`LXP_MODULE_PROGRAMS = 9` in
[`include/layerx/lxp_module.h`](../include/layerx/lxp_module.h)), but a program is not a
ninth economic module and writes no balances of its own. Every monetary effect a program
produces compiles to an authenticated `402LXP` transfer set applied by the kernel — the
same single money doorway every module uses.

Where the rest of the system fits:

- **Identity and authority are kernel**, not a program concern. A program executes inside
  the authority of the activity that invoked it and can never widen it.
- **`402LXP` is the sole balance writer.** Programs emit transfer sets; they do not call
  `set_balance` (no such function exists).
- **Oracle prices** enter through the Crossverse adapter as signed activities, outside
  execution. A state transition never dials out.
- **Settlement** happens on Paxeer Network (EVM chain ID `125`). LayerX orders and
  executes activity; periodic checkpoints settle on Paxeer. Since the monorepo
  integration, the Paxeer settlement stack lives in this same repository under
  [`paxeer-network/`](../paxeer-network) — co-located, but with its own trust and build
  boundary (see [`docs/MONOREPO.md`](../docs/MONOREPO.md) once published, and the root
  [`README.md`](../README.md)).

Normative behavior lives in [`spec/`](../spec) (KVX first). This document is the human
read of the programs workspace and is not a substitute for the spec.

---

## Workspace layout

The programs workspace is a Cargo workspace (`programs/Cargo.toml`) with a strict lint
profile: `unsafe_code` is denied by default, and `unwrap_used`, `expect_used`, and float
arithmetic are denied across the tree.

| Path | Purpose |
| --- | --- |
| `crates/layerx-programs-runtime` | Deterministic WASM runtime: validation, metering, the ABI/capability boundary, cross-program calls, transfers, occupancy accounting, and the FFI bridge into the C kernel |
| `crates/layerx-programs-registry` | Receipt-bound registry: deployment journal, program value-account bindings, real-balance proofs, and wind-down/deprecation |
| `crates/layerx-programs-protocol-adapter` | Thin C↔Rust adapter exposing receipt-verified program state reads to the rest of the protocol |
| `sdk/rust`, `sdk/c`, `sdk/assemblyscript` | Guest program SDKs for the version-one programs ABI, each with a `paid-counter` example |
| `porting/evm`, `porting/solana`, `porting/cosmwasm` | Migration crates and `MIGRATION.md` guides mapping Solidity / Anchor / CosmWasm vocabulary onto the programs ABI |
| `fuzz` | Structure-aware fuzz target and corpus for the runtime |
| `tools` | Boundary scripts: `dependency-policy.sh`, `runtime-module-boundaries.sh` |
| `tests` | Cross-implementation vectors, the hostile-program `gauntlet`, and calldata fixtures |
| `vendor` | Vendored, pinned dependencies for a hermetic build |

### The three crates

**`layerx-programs-runtime`** is the deterministic WASM foundation for guest programs.
It runs on a pinned `wasmi` interpreter with floating point and other nondeterminism
removed. The module map (see `src/lib.rs`) separates concerns deliberately:

- `validate.rs`, `limits.rs` — static module validation and structural bounds (module
  bytes, function count, stack height, call depth).
- `budget.rs`, `meter.rs` — caller-declared activity ceilings, the admitted budget token,
  and per-execution resource metering (CPU fuel, memory, storage read/write, output).
- `abi/` — the transaction boundary. `abi/capability.rs` owns capability grants, their
  canonical encoding, and downward-only narrowing; `abi/response.rs` owns response and
  refusal transport; `abi/storage_ops.rs` owns namespaced storage.
- `accounts.rs` — deterministic derivation of program-owned accounts.
- `transfer.rs`, `ffi_transfer.rs` — the sole monetary exit: typed `402LXP` requests
  bound to invocation authority and submitted as one atomic set to the kernel primitive.
- `occupancy.rs` — deterministic, receipt-bound storage-occupancy accounting.
- `calls.rs`, `engine.rs`, `execute.rs`, `entrypoint.rs`, `lifecycle.rs` — cross-program
  calls, execution, and program lifecycle.
- `host/` — linker orchestration; each host-function family (storage, events, calls,
  transfer) is registered in exactly one unit and reaches execution state only through
  `RuntimeState`.
- `ffi.rs`, `ffi_call.rs` — the FFI bridge the C protocol module calls into.

**`layerx-programs-registry`** (`#![forbid(unsafe_code)]`) turns protocol receipts into
answers about programs: the append-only `DeploymentJournal`, `ProgramValueAccountBinding`
records, and `VerifiedAccountSnapshot`/`ValueAccount` types that resolve a program's real
balance from account-tree Merkle proofs rather than a declared bookkeeping column. It also
owns deprecation and wind-down (`AuthorizedExit`, `ExitRoute`, `WindDownView`).

**`layerx-programs-protocol-adapter`** is the narrow read bridge: `read_program_state`
exposes receipt-verified program state (`ProtocolProgramStateRead`) to the C protocol and
the hosted read surfaces without granting any write authority.

---

## Protocol surfaces

Four surfaces landed with and around the monorepo integration. Each is described here as
it is actually implemented; the authoritative source is the code and
[`spec/layerx-platform`](../spec/layerx-platform) (requirement `37`, tasks `30.1`–`30.5`,
and requirement `36.5` for occupancy).

### Program-owned accounts

A program can own accounts that no principal can claim. An account id is derived
deterministically from the program and a seed — a pure function of public inputs, with no
host state, clock, or entropy involved:

```
account_id = SHA-256(
    "LayerX/programs/program-account/v1\0"   // domain tag, distinct from principal ids
    || program_id                            // 32 bytes, bound before the seed
    || u32_be(seed_len)                      // length prefix removes concatenation ambiguity
    || seed                                  // up to 128 bytes
)
```

The construction is domain-separated: the tag is disjoint from the `LX:ACCOUNT:v1` domain
used for principal/named account ids, so a principal cannot present a public key whose
identifier collides with a derived program account without breaking SHA-256 preimage
resistance. The C kernel (`src/modules/programs/accounts.c`) and the Rust runtime
(`accounts.rs`) agree byte-for-byte, and frozen golden vectors pin the digests.

Deriving an account conveys **no authority**. Program value accounts are registered as
`LX_ACCOUNT_MODULE_VALUE` accounts that carry no authority key, so:

- the ordinary account-open path refuses the `module:programs:value:…` namespace;
- registration is gated on the program owner, not first-caller squatting;
- debits require program authority bound to the deriving program's own frame and exact
  seed (re-derived and checked at transfer time), never a principal signature.

A principal can **fund** a program account (an ordinary `402LXP` credit leg) but can never
**authorize debits** as if it owned the account.

### Downward-only spending grants

Spending authority narrows, never widens. The `ProgramSpend` capability
(`abi/capability.rs`) conveys a bounded grant over accounts the granting program itself
derives — distinct from the `Transfer402` grant over the invoking principal's balance. It
binds `owner_program`, `seed`, `source_account`, `asset`, `to`, and a `maximum_amount`.

Across a program-to-program call edge the grant may only be narrowed:

- any change to identity fields (`owner_program`, `seed`, `source_account`, `asset`, `to`)
  produces a different capability key, so the parent lookup fails and the edge is refused
  with the same typed capability-escalation error principal grants already use;
- an attempt to raise `maximum_amount` above the parent's is refused; a child amount must
  be less than or equal to the parent's;
- only the **owner program** may originate a fresh `ProgramSpend` over its own account on
  an edge; a callee cannot mint authority it was not given.

At transfer time the grant is checked cumulatively against the actual legs, and the
deriving program's frame is the only frame that may stage a program-account debit. There
is no silent widen and no partial transfer set survives a refused escalation.

### Occupancy settlement

State that persists is paid for as long as it persists. Occupancy is deterministic,
batch-indexed rent for persistent program storage — it meters **namespace bytes held
across protocol batches**, priced by the fee schedule. There is no wall-clock component:
"time" here is the protocol batch sequence.

For each occupied storage namespace over a batch interval:

```
byte_batches = recorded_bytes × (to_batch − from_batch)
accrued_fee  = byte_batches × occupancy_byte_batch_price
amount_due   = prior_arrears + accrued_fee
```

Before writing storage a call must establish a signed responsibility mandate that names
the payer and caps both the bytes and the lifetime charge; the occupancy fee budget is
carved out of the activity's signed fee limit, separate from the execution meter. Each
charge resolves to a disposition — `Paid`, `ChargeCeilingExceeded` (namespace frozen),
`ScheduleCeilingExceeded`, `InsufficientFunds`, or `MigrationRequired` (pre-upgrade legacy
bytes, frozen at price zero until the owner migrates). Settlement is charged to the
declared responsible account through ordinary `402LXP` transfer legs and bound into the
batch receipt as canonical, replay-checkable evidence. The principal-scoped namespace is
charged to its principal; a shared namespace is charged to the program owner.

### Protocol-backed program balances

A program's balance is real protocol state, not a registry counter. Registry value
accounts are the same `LX_ACCOUNT_MODULE_VALUE` accounts that live in the kernel account
tree; the registry reads a balance by verifying the account leaf through the account root,
the universal subtree root, and the receipt state root before surfacing it. Wind-down
refuses to strand value: a deprecation cannot complete while a bound account still carries
a non-zero balance without an authorized exit route.

The invariant that ties all of this together: **`402LXP` remains the sole balance
writer.** Programs emit transfer sets — they never set a balance directly. As the platform
spec puts it, *"No program ever receives balance-writing authority. Every monetary effect
a program produces is expressed as authenticated 402LXP transfers applied by the kernel
transfer primitive… a balance change outside a 402LXP transfer aborts the transition."*

---

## SDKs and porting kits

Guest program SDKs target the version-one programs ABI:

- **`sdk/rust`** (`layerx-program-sdk`) — the Rust guest SDK.
- **`sdk/c`** — a C guest SDK with headers, sources, and a toolchain manifest.
- **`sdk/assemblyscript`** — an AssemblyScript SDK (`abi`, `capability`, `transfer`,
  `storage`, `event`, `call`, `receipt` bindings) with a determinism lint.

Each SDK ships a `paid-counter` example: the smallest program that charges for the work it
does and returns a receipt. Note that the newer program-spend capability tag is a
candidate-ABI addition and is not yet mirrored into every SDK; the runtime crate is the
authoritative implementation.

The porting kits map familiar contract vocabularies onto the programs ABI and are explicit
about what does not carry over:

- **`porting/evm`** — porting a Solidity contract.
- **`porting/solana`** — porting a Solana / Anchor program.
- **`porting/cosmwasm`** — porting a CosmWasm contract.

Each has a `MIGRATION.md` written for a developer who already knows the source chain.

---

## Fuzzing, tools, and tests

- **`fuzz/`** — a structure-aware fuzz target (`src/main.rs`) with a checked-in corpus.
- **`tools/dependency-policy.sh`** — enforces the vendored-dependency policy (`deny.toml`).
- **`tools/runtime-module-boundaries.sh`** — enforces the runtime's module-boundary rules.
- **`tests/gauntlet/`** — the hostile-program gauntlet (cross-program derivation, callee
  spend attempts, escalation across depth/fan-out/repeated visits) with an
  `attack-inventory.tsv`.
- **`tests/vectors/`** — cross-implementation vectors, including calldata fixtures, that
  keep the C and Rust surfaces byte-identical.

---

## Building and status

The programs workspace builds with the pinned Rust toolchain declared in
`programs/Cargo.toml` (`rust-version = 1.91.1`) against vendored dependencies. Runtime and
kernel share golden vectors so the C and Rust implementations stay in lockstep. For the
full qualification story — deterministic cross-architecture replay, fault injection,
fuzzing, and settlement — see [`docs/QUALIFICATION.md`](../docs/QUALIFICATION.md).

Project status is deliberately narrow:

| Stage | Detail |
| --- | --- |
| Availability | Limited beta opens **September 7**. |
| Source | Source-available during qualification — built for review now; a broader license follows release qualification. |
| Public endpoints | None yet for LayerX itself. No public RPC, faucet, or explorer. |
| Settlement | Checkpoints settle on Paxeer Network (EVM chain ID `125`); the settlement stack is co-located under `paxeer-network/`. |

A successful local build is development evidence, not authorization to deploy, move
custody, or handle real assets.

---

LayerX is developed by [Sidiora Labs](https://github.com/Sidiora-Labs).
