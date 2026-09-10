# Programs

Programs is kernel module ID `9` (`LXP_MODULE_PROGRAMS`). Activity types occupy
the high 16 bits of `activity_type`, so Programs ordinals are `0x0009xxxx`.
Guest WASM runs through this module. Monetary effects still leave as `402LXP`
transfer sets; the module does not call `set_balance`.

The C kernel lives under `src/modules/programs/`. The deterministic WASM
runtime is `programs/crates/layerx-programs-runtime/`. Receipt-bound registry
reads live in `programs/crates/layerx-programs-registry/`. Bounded leases live
in `programs/crates/layerx-programs-sandbox/`; see [Sandbox](Sandbox.md). Guest
SDKs live under `programs/sdk/`. `platform/sdk/` holds client SDKs (dotnet, go, jvm,
swift, conformance, generators). CALL receipt terminal verification for
those clients is on [SdkTerminalVerification](SdkTerminalVerification.md).

This page is a read of those sources. Where they disagree, both sides are
cited. LXT-20 request codecs and the payments-merchant example are not in this
tree; see
[Payments developer path](PaymentsQuickstart.md) and [Assets](Assets.md).

Sources:

- `include/layerx/lxp_module.h` (`LXP_MODULE_PROGRAMS = 9`)
- `include/layerx/programs.h` (`LX_PROGRAMS_DEPLOY` … `LX_PROGRAMS_SANDBOX_DESTROY`)
- `src/modules/programs/registration.c` (`programs_module_registration`, `_v2`, `_v3`, `_v4`)
- `programs/README.md`

---

## Activity kinds

Dispatch is in `programs_decode` / `programs_validate` / `programs_execute`
(`src/modules/programs/registration.c`). Ordinal 1 and 2 are lifecycle
(`lxp_programs_lifecycle_*` in `deploy.c`). Ordinal 3 is CALL
(`lxp_programs_call_*` in `call.c`). Remaining ordinals have dedicated
controllers.

| Type constant | Value | Ordinal | Controller |
| --- | --- | ---: | --- |
| `LX_PROGRAMS_DEPLOY` | `0x00090001` | 1 | `deploy.c` lifecycle |
| `LX_PROGRAMS_UPGRADE` | `0x00090002` | 2 | `deploy.c` lifecycle |
| `LX_PROGRAMS_CALL` | `0x00090003` | 3 | `call.c` |
| `LX_PROGRAMS_REGISTRY` | `0x00090004` | 4 | generic KV record in `registration.c` |
| `LX_PROGRAMS_TRANSFER` | `0x00090005` | 5 | `transfer.c` |
| `LX_PROGRAMS_ACCOUNT` | `0x00090006` | 6 | `accounts.c` (module ABI ≥ 2) |
| `LX_PROGRAMS_WIND_DOWN` | `0x00090007` | 7 | `winddown.c` (module ABI ≥ 2) |
| `LX_PROGRAMS_FEE_GOVERNANCE` | `0x00090008` | 8 | fee governance (module ABI ≥ 2) |
| `LX_PROGRAMS_SANDBOX` | `0x00090009` | 9 | `sandbox.c` (module ABI ≥ 3) |
| `LX_PROGRAMS_SANDBOX_DESTROY` | `0x0009000A` | 10 | `sandbox_expiry.c` (module ABI ≥ 4) |

Which types are advertised depends on the registered module ABI, not on the
guest ABI:

- v1 (`LX_PROGRAMS_ABI_VERSION = 1`): DEPLOY, UPGRADE, CALL, REGISTRY, TRANSFER
- v2 (`LX_PROGRAMS_ACCOUNT_ABI_VERSION = 2`): adds ACCOUNT, WIND_DOWN, FEE_GOVERNANCE
- v3 (`LX_PROGRAMS_SANDBOX_ABI_VERSION = 3`): adds SANDBOX
- v4 (`LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION = 4`): adds SANDBOX_DESTROY

Genesis registers Programs v4 for every accepted protocol version
(`src/protocol/lxp_genesis.c:594-598`). Asset v1 is registered only when
`protocol_version` is 3. Library default `LXP_PROTOCOL_VERSION` is occupancy
protocol 2.

### DEPLOY

`lxp_programs_lifecycle_decode` ordinal 1. Payload starts with 32-byte
`program_id`, then `abi_version` (u16), policy byte (`0` immutable / `1`
authority), reserved zero, 32-byte upgrade authority (all-zero iff immutable),
32-byte code hash, u32 wasm length, optional framed interface, then WASM.

`execute_deploy` refuses an existing program key (`LXP_ERR_SEQUENCE_REUSED`),
stores the artifact, writes a 71-byte program record, optionally stores the
interface, and emits event type 1 (`PROGRAM_EVENT_DEPLOYED` / `LX_PROGRAMS_EVENT_DEPLOYED`)
with the new code hash. Module ABI 2 or 3 binds the deployer as owner
unconditionally; ABI 4 binds only when `protocol_version == 3`. DEPLOY is
refused with `LXP_ERR_VERSION_UNSUPPORTED` on ABI 4 outside protocol 3
(`deploy.c:400-409`).

WASM must start with `\0asm` version 1 and SHA-256 to the declared hash
(`validate_wasm`).

### UPGRADE

Ordinal 2. Payload: `program_id` (0), `abi_version` u16 (32), flags byte (34;
bit0 = hook present, bit1 = interface drop), reserved zero byte (35), old
hash (36), new hash (68), `hook_length` u16 (100, always present, non-zero
iff bit0), `wasm_length` u32 (102), optional `interface_length` u32 (106),
then hook bytes, interface bytes, WASM. The length field is fixed; the hook
bytes are optional. `execute_upgrade` requires the stored
policy to be authority (`PROGRAM_POLICY_AUTHORITY`), the activity principal to
match the stored authority, and `old_hash` to match the record. ABI transitions
must be monotonic (`lxp_programs_abi_transition_validate`). Version counter at
record offset 67 increments. Event type 2 carries old hash then new hash.

A CALL against a deprecated or tombstoned program is refused (see lifecycle).

### CALL

`lxp_programs_call_decode` requires at least `PROGRAM_CALL_FIXED_BYTES`
(32 + 2 + 2 + 4 + 2 + 4 + 4 + 7×8). Layout: `program_id`, guest `abi_version`,
entrypoint length, calldata length, capabilities length, access-declaration
length, response capacity, then seven u64 budget fields, then the four
variable spans. Trailing bytes are an error. Entrypoint bytes are
`[A-Za-z0-9_.]+`.

Validate loads the 71-byte program record, requires guest ABI to match the
record, requires `lxp_programs_program_active`, and opens the artifact by
`(program_id, code_hash)`. Execute binds occupancy (when the protocol uses
occupancy) and hands the arena-owned activity to the Rust CALL FFI
(`layerx_programs_call_begin` via `call_scalar_begin`).

Guest ABI 0 is refused. Guest ABI above the registered module ABI, or above 2,
is `LXP_ERR_VERSION_UNSUPPORTED`. Guest ABI 2 additionally requires
`lxp_protocol_version_uses_occupancy` (protocol 2 or 3).

### Simulate is not an activity type

Simulate is LNI capability `simulate` (request tag 30 / response tag 31,
LNI minor ≥ 4). `lxp_daemon_lni_simulate` decodes one signed activity, refuses
anything other than `LX_PROGRAMS_CALL`, prepares a one-activity batch against
the current head, encodes the receipt plus terminal and call-graph, signs
simulation evidence, then `lxp_kernel_prepared_batch_destroy` and
`lxp_arena_reset`. The durable log, sequence, and occupancy ledger are not
committed.

Hosted routes `POST /v1/programs/call` and `POST /v1/programs/simulate` carry
the same CALL bytes over LNI; only simulate uses the non-committing path.

Sources:

- `include/layerx/programs.h:165-179`
- `src/modules/programs/registration.c:17-52, 62-199, 216-290`
- `src/modules/programs/deploy.c` (`decode_deploy`, `decode_upgrade` at 264-306, `execute_deploy` at 400-409, `execute_upgrade`)
- `src/protocol/lxp_genesis.c:594-598`
- `src/modules/programs/call.c` (`lxp_programs_call_decode`, `lxp_programs_call_validate`, `lxp_programs_call_execute`)
- `cmd/layerxd/lxp_daemon_lni.c` (`lxp_daemon_lni_simulate`, `send_simulate`)
- `agent/schema/lni/README.md` (LNI 1.4 `simulate`)

---

## LXT-20

`programs/sdk/rust/src/lxt20.rs`, `programs/sdk/rust/examples/token-lxt20`, and
`programs/fixtures/pay5` are not in this tree; the contract below is the
reference the Programs ABI admits, not a path that can be built here.

The Rust guest SDK's LXT-20 example is an ABI-v2 state machine backed by one
native Asset. Token balances and allowances are program storage; the backing
units move only through `402LXP`. The reference program fixes its Asset,
supply, and ceiling at build time. It has no mint, burn, permit, or nested-call
surface.

### Interface and calldata

The native Programs interface publishes eight entries:

| Method | Selector | Payload |
| --- | --- | --- |
| `initialize` | `4c 58 14 00` | empty |
| `transfer` | `4c 58 14 01` | recipient id32, amount `u128` |
| `approve` | `4c 58 14 02` | spender id32, amount `u128` |
| `transfer_from` | `4c 58 14 03` | owner id32, recipient id32, amount `u128` |
| `balance_of` | `4c 58 14 04` | owner id32 |
| `allowance` | `4c 58 14 05` | owner id32, spender id32 |
| `total_supply` | `4c 58 14 06` | empty |
| `metadata` | `4c 58 14 07` | empty |

Canonical requests are:

```text
selector4 || [version=1, width=0x20] || payload_length:u32_be || payload
```

The exact payload lengths are 48 for `transfer`/`approve`, 80 for
`transfer_from`, 32 for `balance_of`, 64 for `allowance`, and zero for
`initialize`, `total_supply`, and `metadata`. Transfer amounts are positive;
approval of zero is valid and revokes the allowance. The total request is at
most 90 bytes.

Responses are `[1,0x20] || payload_length:u32_be || payload`. Balance,
allowance, and total supply return one `u128`; mutation calls return an empty
payload; metadata is the fixed configured value. `transfer_from` always
subtracts the exact amount, including when the allowance is the maximum
`u128`.

### Account and deployment preparation

`PreparedProgramAccount` derives an account from:

```text
SHA-256("LayerX/programs/program-account/v1\\0"
       || program_id32 || seed_length:u32_be || seed)
```

Its native account-registration payload is:

```text
program_id32 || "LXPA1" || asset_id32 || seed_length:u32_be || seed
```

Register the program account for the chosen Asset before funding it. Funding
uses an ordinary principal-funded `Transfer402` grant; spending uses the
separate bounded `ProgramSpend` capability. Derivation is not registration,
funding, or debit authority.

Deploy with ABI version 2 and include the generated interface that binds all
eight selectors, input/output bounds, and transfer capability offsets. Because
`transfer` and `transfer_from` carry `CallerAuthorizedSpend`, the canonical
interface uses `LayerX/program-interface/v2\0`; the v1 interface encoding and
guest ABI 1 refuse that descriptor
(`programs/crates/layerx-programs-registry/src/interface.rs:13-14, 306-320,
820-835`).

The reference descriptor binds the backing Asset and per-call ceiling, with
recipient/amount offsets `10`/`42` for `transfer` and `42`/`74` for
`transfer_from`
(`programs/crates/layerx-programs-registry/src/lxt20.rs:68-70, 89-100`). It
does not grant spending authority. Admission extracts the recipient and u128
amount from canonical calldata, refuses zero or over-ceiling values, and
requires a matching caller `ProgramSpend` grant bound to this program, its
derived source account, Asset, recipient, and amount
(`programs/crates/layerx-programs-runtime/src/dynamic_spend.rs:11-47`). A
registry source/state record alone is not proof that the native deploy
activity executed; require its verified receipt.

The initialization request is:

```text
4c581400012000000000
```

Initialization atomically stages the full fixed supply from the configured
issuer to the registered program account. A recipient must call `approve`,
including zero, at least once so its derived program-account storage exists
before a transfer credits it.

### Calls and receipt reads

Build the reference guest with:

```sh
cargo build \
  --manifest-path programs/sdk/rust/examples/payments-merchant/Cargo.toml \
  --target wasm32-unknown-unknown --release
```

Submit deploy and call activities through the native Programs routes or
`lx_sendActivity`, retain the activity id, and require the requested commitment
evidence. A call's transfer grants must match the generated interface and the
registered program account; any capability widening or unmatched dynamic
spend is refused atomically.

Guest `receipt_read` does not expose raw kernel state. The host writes exactly
116 bytes for an explicitly granted digest:

```text
receipt_digest32
|| result_code:i32_be
|| asset_id32
|| amount:u128_be
|| state_root32
```

The SDK refuses any other length, malformed field, or digest different from
the requested receipt. The merchant example therefore consumes verified,
explicitly granted payment evidence rather than trusting a caller-supplied
receipt description.

When a principal or program-funding leg resolves a named native account, the
runtime wraps the original transfer authorization with
`LayerX/programs/402LXP/account-bound-set/v1\0`, the u32 length and bytes of
that authorization, then one u16-length-prefixed account name per leg. Program
spend legs require an empty name; principal and funding legs recompute the
source account from the supplied canonical name. The resulting 115-byte kernel
legs and transfer root therefore commit the actual account endpoints while the
original authorization retains the signer principal and invocation authority
(`programs/crates/layerx-programs-runtime/src/transfer.rs:882-918, 921-955`).
Nested wrappers, trailing data, forged names, or a recomputed root mismatch are
refused. The positive native per-Asset evidence and negative vectors for that
refusal live in `programs/fixtures/pay5/account-authorization-vectors.json`,
which is not in this tree.

---

## Guest ABI v2 and protocol 3

Three version numbers are not the same thing.

| Number | What it versions | Values in tree |
| --- | --- | --- |
| Guest ABI | WASM import surface a program is compiled against | Frozen 1 and 2 (`programs/abi-frozen.sha256`) |
| Module iface ABI | Which Programs activity types the kernel registers | 1–4 (`LX_PROGRAMS_*_ABI_VERSION`) |
| Protocol version | Envelope / occupancy / state-commitment | 1 legacy, 2 occupancy, 3 state commitment |

### Guest ABI

Crate-root `ABI_VERSION` is 2. ABI 1 is the frozen `layerx_v1` host
table: `storage_read`, `storage_write`, `storage_delete`, `event_emit`,
`program_call`, `transfer_402`, `receipt_read`. ABI 2 keeps that namespace and
adds `layerx_v2`: response/refusal, scoped storage including scan and drop,
`transfer_program_402`, `fund_program_402`, `context_read`, `balance_read`,
hash, signature verify/recover, and 256-bit bigint ops. Bounded
`storage_scan_scoped` encoding, ceilings, and refusals are on
[StorageScan](StorageScan.md).

Rust `admit_abi_version` accepts only 1 and 2. C CALL decode also caps guest
ABI at 2. C `lxp_programs_abi_transition_validate` additionally accepts
requested `LX_PROGRAMS_SANDBOX_ABI_VERSION` (3). That C lifecycle admission is
wider than the frozen guest ABI and the Rust runtime.

ABI 1 CALL is admitted on protocol 1, 2, or 3. ABI 2 CALL requires occupancy
protocol 2 or 3 (`protocol_admits_abi` in `ffi_call.rs`; same predicate in
`lxp_programs_call_decode`).

C guest SDK header `programs/sdk/c/include/layerx/program.h` defines
`LXP_PROGRAM_ABI_VERSION = 1` and `LXP_PROGRAM_ABI_MODULE "layerx_v1"`. ABI 2
capability tags `ProgramSpend` (9) and `BalanceView` (10) are documented in
`programs/README.md` as ABI-2-only.

### Protocol 3

`LXP_PROTOCOL_VERSION_STATE_COMMITMENT = 3`. Occupancy is used by both 2 and 3
(`lxp_protocol_version_uses_occupancy`). Header default `LXP_PROTOCOL_VERSION`
is 2. Hosted testnet pins wire protocol 3
(`platform/hosted/testnet/deployment.yaml:10`,
`lxp-wire-protocol-version: "3"`). Beta-cluster genesis writes
`protocol_version: 3` (`platform/hosted/tests/beta-cluster.sh:794`). Genesis
builder accepts protocol 2 or 3 and registers Asset v1 when protocol is 3
(`cmd/layerx-genesis/lxp_genesis_builder.c:88,152`). Bridge credit
`validate_credit` requires `activity->protocol_version != 3U` to fail.
The hosted readiness response reports its `wire_version`; it does not select
the protocol used by genesis.

Protocol 3 receipt encoding carries occupancy fields on program outcomes with
`encoding_version >= 2`. `layerx_programs_call_terminal_publish` writes
`encoding_version = 3`.

Sources:

- `include/layerx/lxp_protocol.h:8-12`
- `src/protocol/lxp_protocol.c:35-61`
- `programs/crates/layerx-programs-runtime/src/lib.rs` (`ABI_VERSION`, `ABI_MANIFEST`)
- `programs/crates/layerx-programs-runtime/src/abi/manifest.rs`
- `programs/crates/layerx-programs-runtime/src/abi_policy.rs`
- `programs/crates/layerx-programs-runtime/src/ffi_call.rs` (`protocol_admits_abi`)
- `src/modules/programs/deploy.c:84-96` (`lxp_programs_abi_transition_validate`)
- `src/modules/programs/call.c:1703-1707, 1029`
- `programs/abi-frozen.sha256`
- `programs/sdk/c/include/layerx/program.h:10-21`
- `platform/hosted/testnet/deployment.yaml:10`
- `platform/hosted/tests/beta-cluster.sh:794`
- `cmd/layerx-genesis/lxp_genesis_builder.c:88,152`
- `src/protocol/lxp_genesis.c:594-598`

---

## Program records and lifecycle states

### Deployed program record

Key prefix `"program\0"` plus 32-byte program id (40 bytes). Value is 71 bytes:

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | policy (`0` immutable, `1` authority) |
| 1 | 32 | upgrade authority |
| 33 | 32 | current code hash |
| 65 | 2 | guest ABI version |
| 67 | 4 | version counter (DEPLOY writes `1`) |

Interface records, when present, use prefix `"interface\0"` plus program id.

WASM bytes are blobs addressed by `(program_id, code_hash)`
(`lxp_programs_artifact_store` / `lxp_programs_artifact_open`). Snapshots carry
those blobs in the kernel-blob section so restore reproduces the Programs
subtree root and a post-restore CALL resolves the same artifact.

### Wind-down states

`lx_programs_lifecycle_status`: `ACTIVE = 1`, `DEPRECATED = 2`,
`TOMBSTONED = 3`. Absence of a wind-down status record means active.
`lxp_programs_program_active` returns `LXP_OK` on `LXP_ERR_UNKNOWN_FIELD` and
`LXP_ERR_PROGRAM_REFUSED` when a deprecation or tombstone record exists.
CALL validate and UPGRADE execute both call it.

WIND_DOWN is a separate activity (ordinal 7). The registry crate models the
same states as `Deprecation`, `AuthorizedExit`, `ExitRoute`, `WindDownView`
over receipt-verified account proofs. A deprecation cannot complete while a
bound value account still carries a non-zero balance without an authorized
exit.

Sources:

- `src/modules/programs/deploy.c:12-33, 98-109, 366-417, 420-496`
- `include/layerx/programs.h:248-252`
- `src/modules/programs/winddown.c:204-211`
- `src/modules/programs/call.c:1751-1757`
- `programs/crates/layerx-programs-registry/src/lib.rs` (exports)
- `programs/crates/layerx-programs-runtime/src/lifecycle.rs` (`UpgradePolicy`, `Deploy`, `Upgrade`)

---

## Occupancy and why CALL settlement depends on batch finality

Occupancy meters namespace bytes held across protocol batches. Time is the
batch number, not a wall clock. Keys: `"progocc/head/v3"`, `"progocc/final/v3"`,
ledger chunks `"progocc/l/v3/"`. Receipt domain
`LXP/programs/occupancy-receipt/v2`.

On CALL execute, `lxp_programs_occupancy_bind_call` carves occupancy out of
the signed fee limit after pricing the six execution-budget fields. A success
terminal is a fatal invariant if occupancy protocol is on and
`occupancy->applied` is false (`layerx_programs_call_terminal_begin`).

`applied` is set when occupancy output is persisted and any due legs are
emitted through `lxp_ctx_emit_programs_maintenance_transfer_set`. Batch close
is a different seam: replay registration requires a batch finalizer on
occupancy protocols (`lxp_replay_engine_register`). That finalizer is
`lxp_programs_replay_finalize` → `lxp_programs_finalize_occupancy_batch`.

Finalize requires `batch_number == finalized_batch + 1` (`LXP_ERR_BATCH_GAP`
or `LXP_ERR_IDEMPOTENT_REPLAY` otherwise), opens a journal, runs sandbox
expiry, then the Rust occupancy finalize. Occupancy charges for bytes that
*persist across batches*, so a CALL that writes storage is not fully settled
until the batch that contains it is finalized. Simulate destroys the prepared
batch, so it never reaches this finalizer.

CALL success outcomes with outcome `encoding_version >= 2` copy occupancy
byte-batches, fee units, asset id, evidence digest, and occupancy transfer
root from the occupancy receipt into the program outcome.

Sources:

- `src/modules/programs/occupancy.c` (`lxp_programs_occupancy_bind_call`, `lxp_programs_finalize_occupancy_batch`, `lxp_programs_replay_finalize`)
- `src/modules/programs/occupancy.h` (`finalized_batch`, `applied`, `call_authorized`)
- `src/modules/programs/call.c:927-930, 1048-1061, 1915-1919`
- `src/replica/lxp_replay.c:79-81`
- `programs/README.md` (occupancy settlement)

---

## Receipts and terminals

A CALL terminal is begun with kind `SUCCESS` (1), `FAILURE` (2), or `RESOURCE`
(3) (`lxp_program_terminal_kind`). Success requires `result_code == LXP_OK`;
non-success forbids `LXP_OK` and forbids fatals. Three reserved buffers -
call graph, terminal payload, events - are hashed on publish.

`layerx_programs_call_terminal_publish` binds `lxp_program_outcome` onto the
activity context. Only success emits `CALL_OUTCOME` and copies the transfer
root. Failure/resource bind the outcome without that event.

### Guest ABI 1: principal-only transfer sources

`layerx_programs_call_transfer_leg` source kinds:

- `PROGRAM_TRANSFER_SOURCE_PRINCIPAL = 1`
- `PROGRAM_TRANSFER_SOURCE_PROGRAM = 2`
- `PROGRAM_TRANSFER_SOURCE_PROGRAM_FUNDING = 3`

Kind 1 requires `from == authority->principal`, zero `owner_program`, and no
seed. Kinds 2 and 3 require `value->abi_version == LX_PROGRAMS_ACCOUNT_ABI_VERSION`
(guest ABI 2). ABI 1 therefore cannot debit a program-owned account or fund
one through this CALL transfer path.

### Guest ABI 2: authority attachment

`layerx_programs_call_transfer_apply` validates every leg, then attaches a
`lxp_transfer_source_authority` per distinct `from` (`authorized_from`,
`debit_authority_kind = LXP_AUTH_OWNER`) onto `set->context.source_authorities`
before `lxp_ctx_emit_transfer_set`.

Kind 2 (program spend): `from` must be the derived module-value account for
`(owner_program, seed)`; that account has no authority key; owner must equal
staging program. Kind 3 (program funding): `from` is still the principal;
`to` is the derived program value account.

The 402 context `authorized_from` remains the activity principal. Extra
source authorities are how ABI 2 debits program-owned accounts without giving
the guest `set_balance`.

Sources:

- `include/layerx/lxp_receipt.h:34-72`
- `src/modules/programs/call.c:25-29, 887-1092, 1369-1641`
- `programs/crates/layerx-programs-runtime/src/ffi_call.rs` (CALL FFI)

---

## Events (`CALL_OUTCOME`)

Programs publication events used by CALL:

| Constant | Value | Emitter |
| --- | ---: | --- |
| `LX_PROGRAMS_EVENT_GUEST_ENVELOPE` | 6 | `lxp_programs_emit_guest_event` |
| `LX_PROGRAMS_EVENT_CALL_OUTCOME` | 7 | `lxp_programs_emit_call_outcome` |

Guest envelopes: domain `LXGE`, envelope version 1, kind 1, 185-byte body.
Topic/data are stored as domain-separated SHA-256
(`LayerX/programs/event-topic/v1`, `LayerX/programs/event-data/v1`). Max 64
events per CALL (`LXP_PROGRAMS_EVENT_MAX_COUNT`). Principal in the envelope
must equal the admission payer.

`CALL_OUTCOME` uses domain `LXMO`, outcome envelope version 2, kind 2,
255-byte body (metering schedule version included). Legacy 251-byte `LXCO`
bodies are still accepted by `envelope_effect_valid`. Emit requires
`terminal_result == LXP_OK`. Body binds program id, principal, activity id,
frame, runtime/ABI/fee/metering versions, transfer-set root, call-graph
digest, terminal-payload digest, event-envelope digest.

`lxp_programs_project_committed_events` (`event.c:238-286`) filters those two
Programs event types and orders them by increasing ordinal.
`lxp_programs_project_receipt_events` (`event.c:288-317`) treats guest-envelope
or `CALL_OUTCOME` events without `program_outcome.present` as non-canonical.

DEPLOY/UPGRADE still emit event types 1 and 2 from `deploy.c` (code hashes),
which are not in this publication seam.

Sources:

- `include/layerx/programs.h:180-191`
- `src/modules/programs/event.h`
- `src/modules/programs/event.c:238-286` (`lxp_programs_project_committed_events`)
- `src/modules/programs/event.c:288-317` (`lxp_programs_project_receipt_events`)
- `src/modules/programs/call.c:1116-1211, 1004-1092`

---

## Custody credit and program funding

Genesis registers Programs v4 on every accepted protocol version; Asset v1 is
protocol-3-conditional (`src/protocol/lxp_genesis.c:594-598`). A CALL needs a
funded fee-paying principal. When a bridge profile is present, genesis writes
a zero `custody-issued:` supply key (`src/protocol/lxp_genesis.c:549-553`).

Custody credit is Bridge module 8 ordinal 1:
`LXP_BRIDGE_CREDIT = (8U << 16U) | 1U`. It exists only when genesis carries
the 207-byte profile under key `custody-credit-profile/v1` (`LXBC1` … protocol
bytes `0, 3`). Profile genesis requires protocol 3
(`lxp_bridge_genesis_profile`). Credit verify and credit validate also
require protocol 3. The activity actor must be `LXP_AUTHORITY_OWNER`.

Credit does not preallocate balances. It issues against a finalized external
deposit into the pinned vault, then `lxp_ctx_bridge_credit`
(`src/protocol/lxp_module_ctx.c:1215`) credits the beneficiary main account via
`402LXP`. That account can then pay CALL fees and ABI-2 `PROGRAM_FUNDING`
legs into `module:programs:value:<account-id>` accounts. A principal can
fund a program account; it cannot authorize debits from one (ABI-1 has no
program-source kind; ABI-2 program debits require the deriving program's
frame and seed).

See [Custody](Custody.md) for the profile layout and evidence path.

Sources:

- `include/layerx/lxp_bridge_credit.h` (`LXP_BRIDGE_CREDIT`, sizes)
- `src/modules/bridge/lxp_bridge_credit.c` (`lxp_bridge_profile_key`, `lxp_bridge_genesis_profile`, `validate_credit`, `lxp_bridge_module_iface`)
- `src/protocol/lxp_module_ctx.c:1215` (`lxp_ctx_bridge_credit`)
- `src/protocol/lxp_genesis.c:549-553, 594-598`
- `src/modules/programs/call.c:1520-1565` (`PROGRAM_TRANSFER_SOURCE_PROGRAM_FUNDING`)
- `src/modules/programs/accounts.c` (program account derivation / lookup)
- `docs/wiki/Custody.md`

---

## Frozen-ABI drift gates

`programs/abi-frozen.sha256` pins two vectors:

```
1 09fcad46aeea9659d7d555a4a09ec151bd36d60b85bfd052d1f7a43cf71d58a3
2 8827869bf1324c3e82c60baf7360b1c3e2baa84d8591e49eb6b73c2cea6c8b69
```

`make programs-abi-drift` runs `programs/tools/check-abi-drift.sh`, which
executes `programs/tools/generate-abi-vectors.py --check`. The generator
rebuilds the v1 and v2 manifests from `abi/manifest.rs`, `abi/mod.rs`,
`lib.rs`, and `programs/sdk/rust/src/abi.rs`, and fails if they diverge from
the frozen checksums or from each other (v1 table vs v1 manifest; v1+v2
tables vs composite v2 manifest; v2 function types vs signatures). Crate-root
`ABI_VERSION` must remain 2. The file is updated only when a newly allocated
ABI version is frozen.

`make programs-test` depends on `programs-abi-drift`.

Sources:

- `programs/abi-frozen.sha256`
- `programs/tools/check-abi-drift.sh`
- `programs/tools/generate-abi-vectors.py`
- `Makefile` (`programs-abi-drift`, `programs-test`)

---

[Home](Home.md)
