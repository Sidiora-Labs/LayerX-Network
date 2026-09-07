# Sandbox

`programs/crates/layerx-programs-sandbox/` is the bounded-lease state
machine, lease-scoped guest execution, and renter-authorized snapshot
path. This page is those three surfaces.

---

## Lease states and transitions

`LeaseState` tags (`programs/crates/layerx-programs-sandbox/src/lease.rs:277-285`):

| State | Tag |
| --- | ---: |
| `Requested` | 0 |
| `Funded` | 1 |
| `Active` | 2 |
| `Settling` | 3 |
| `Expired` | 4 |
| `Destroyed` | 5 |

`LeaseState::is_terminal` is true only for `Destroyed`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:287-291`).
`Lease::request_with_schedule` constructs `Requested`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:937`).

`LeaseActivity` tags (`programs/crates/layerx-programs-sandbox/src/lease.rs:294-305`):

| Activity | Tag |
| --- | ---: |
| `Request` | 0 |
| `Fund` | 1 |
| `Activate` | 2 |
| `BeginSettlement` | 3 |
| `Expire` | 4 |
| `Destroy` | 5 |
| `CloseBoundExceeded` | 6 |
| `Snapshot` | 7 |

`declared_edge` admits only these triples
(`programs/crates/layerx-programs-sandbox/src/lease.rs:307-356`):

| Activity | From | To | Principal slot |
| --- | --- | --- | --- |
| `Request` | `Requested` | `Requested` | acquire |
| `Fund` | `Requested` | `Funded` | hold |
| `Activate` | `Funded` | `Active` | hold |
| `BeginSettlement` | `Active` | `Settling` | hold |
| `CloseBoundExceeded` | `Active` | `Settling` | hold |
| `Snapshot` | `Active` | `Active` | hold |
| `Expire` | `Requested` | `Expired` | release |
| `Expire` | `Funded` | `Expired` | release |
| `Expire` | `Active` | `Expired` | release |
| `Expire` | `Settling` | `Expired` | release |
| `Destroy` | `Expired` | `Destroyed` | none |

`LeaseBook` stores `leases` and `active_by_principal`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1667-1671`).
`MAX_CONCURRENT_LEASES_PER_PRINCIPAL` is `32`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:30`).
`insert_requested` refuses `count >= 32` as
`LeaseRefusal::PrincipalLeaseLimit`, then stores `count + 1`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1682-1704`,
`programs/crates/layerx-programs-sandbox/src/lease.rs:1760-1766`).

`LeaseBook::transition` treats a lease as holding a slot when its state
is not `Expired` and not `Destroyed`. After a successful transition, if
that lease held a slot and the new state is `Expired` or `Destroyed`,
it subtracts one from `active_by_principal`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1725-1744`).
`Lease::transition` refuses `Request` and `CloseBoundExceeded` as
`IntrinsicActivityRequired` and refuses `Destroy` as `StorageRequired`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1442-1450`). The
only `LeaseBook::transition` edge that therefore releases a slot is
`Expire` into `Expired`.

`LeaseBook::destroy_with_evidence` applies `Destroy` and does not change
`active_by_principal`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1747-1756`).
`Expire` has already released the slot.

`Expire` from a state other than `Settling` requires
`evidence.batch_sequence >= expiry` or it is `NotExpired`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1582-1589`).
`Expire` from `Settling` does not use that bound.

`CloseBoundExceeded` is not a `Lease::transition` activity. `record_usage`
applies it when usage first exceeds a limit, lifetime, or escrow, and
returns `UsageOutcome::ClosedByBound`
(`programs/crates/layerx-programs-sandbox/src/lease.rs:1602-1663`). Direct
`LeaseBook::transition` of that activity is `IntrinsicActivityRequired`.

Tests:

- `transition_matrix_refuses_every_undeclared_edge` asserts
  `declared_edge` on every activity/from/to triple, including that no
  activity leaves `Destroyed` for `Funded`
  (`programs/crates/layerx-programs-sandbox/src/lease.rs:2052-2126`).
- `real_receipts_drive_every_lifecycle_edge_and_destroyed_never_revives`
  drives receipt-backed `Request` → `Fund` → `Activate` →
  `BeginSettlement` → `Expire` → `Destroy`, refuses replayed evidence,
  and refuses `Fund` from `Destroyed` as `InvalidTransition`
  (`programs/crates/layerx-programs-sandbox/tests/lease.rs:166-279`).
- `real_bound_receipt_closes_intrinsically_and_refuses_mismatch_and_regression`
  records in-limit usage, refuses a regression, refuses
  `CloseBoundExceeded` through `transition` as
  `IntrinsicActivityRequired`, and closes through `record_usage` as
  `ClosedByBound`
  (`programs/crates/layerx-programs-sandbox/tests/lease.rs:292-433`).
- `real_request_evidence_enforces_principal_concurrency_and_expiry`
  admits 32 leases for one principal, refuses the 33rd as
  `PrincipalLeaseLimit`, `Expire`s one `Requested` lease, admits a
  replacement, then `Destroy`s the expired lease and still refuses a
  further insert as `PrincipalLeaseLimit`. After every remaining lease
  is expired and destroyed, 32 new leases admit and the next is again
  `PrincipalLeaseLimit`
  (`programs/crates/layerx-programs-sandbox/tests/lease.rs:436-625`).
- `principal_concurrency_and_declaration_bounds_are_enforced` asserts
  `ensure_principal_capacity(31) == Ok(())` and
  `ensure_principal_capacity(32) == Err(PrincipalLeaseLimit)`
  (`programs/crates/layerx-programs-sandbox/src/lease.rs:2129-2137`).

---

## Capability refusals

`LeaseCapabilities::derive` builds the only authority a sandbox image
receives: the lease-namespace execution principal, that principal's
storage namespace, and `CapabilitySet::new([StorageRead, StorageWrite])`
(`programs/crates/layerx-programs-sandbox/src/execute.rs:26-41`). Shared
storage, transfer, balance, receipt, event, and callee authority are not
admitted (`programs/crates/layerx-programs-sandbox/src/execute.rs:22-25`).

`capabilities_are_derived_and_contain_no_escape_authority` asserts the
grant encoding `vec![0, 2, 1, 2]`
(`programs/crates/layerx-programs-sandbox/src/execute.rs:352-365`).
`hostile_authority_families_are_absent_by_construction` asserts that
tags `3`, `4`, `5`, `6`, `7`, `8`, `9`, and `10` are absent from that
encoding (`programs/crates/layerx-programs-sandbox/src/execute.rs:376-381`).

`hostile_images_cannot_emit_or_call_an_unleased_program` instantiates a
validated guest image that imports one host function and calls it from
`CALL_ENTRY_EXPORT` under `LeaseCapabilities` authorization
(`programs/crates/layerx-programs-sandbox/src/execute.rs:399-481`).
Storage is cloned before the call and compared after; both refusals
leave storage equal to that clone
(`programs/crates/layerx-programs-sandbox/src/execute.rs:453-480`).

| Host import | Input | Typed error |
| --- | --- | --- |
| `event_emit` | arity 4, `i32` zeros, `ABI_MODULE` import | `ExecutionError::Entrypoint(EntrypointRefusal::GuestRefused { code: -1 })` |
| `program_call` | arity 6, callee id `[8; 32]` in data, `ABI_MODULE` import | `ExecutionError::Composition(CompositionRefusal::Authority(AbiError::CapabilityDenied))` |

(`programs/crates/layerx-programs-sandbox/src/execute.rs:422`,
`programs/crates/layerx-programs-sandbox/src/execute.rs:434-478`).

---

## Snapshot persist and restore metering

`SNAPSHOT_CHUNK_BYTES` is `65_536`
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:18`).
`SandboxState::canonical_bytes` encodes domain
`LayerX/programs/sandbox/snapshot/v1\0`, source lease id, host program
id, namespace prefix, length-prefixed linear memory, globals,
continuation, and ordered namespace cells
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:16`,
`programs/crates/layerx-programs-sandbox/src/snapshot.rs:207-231`).

`Snapshot::commit` sets `byte_length` to that canonical encoding's
length and SHA-256s it as the snapshot digest
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:379-383`).
`snapshot_entries` persists a 44-byte manifest (digest, `u64` length,
`u32` chunk count) plus `bytes.chunks(65_536)`
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:676-707`).
`storage_bytes` is `entries_metered_bytes` of those entries: each key
and value summed through `storage::metered_bytes`
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:432`,
`programs/crates/layerx-programs-sandbox/src/snapshot.rs:710-718`).
`commit_snapshot_storage` is charged `storage_bytes`
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:433-435`).
Namespace occupancy after persist is live-namespace persistent bytes
plus protocol prefix bytes under `b"snapshot"`; exceeding
`lease.limits().namespace_bytes` is `TargetBoundExceeded`
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:412-431`).

`restore` requires a matching `snapshot_records` digest, a `Funded`
target, owner principal, host program, and image hash
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:520-536`).
It charges
`charge_storage_write(byte_length + entries_metered_bytes(rebound live cells))`
before instantiate
(`programs/crates/layerx-programs-sandbox/src/snapshot.rs:568-597`).
`instantiate_sandbox` and `restore_continuation` then run against that
meter (`programs/crates/layerx-programs-sandbox/src/snapshot.rs:611-616`).

The restore fixture sets `reconstructed_bytes = snapshot.byte_length() + 15`
and `total_restored_bytes = reconstructed_bytes + 65_536 + 16`
(`programs/crates/layerx-programs-sandbox/tests/snapshot.rs:507-508`).
Persist `storage_write_bytes >= snapshot.storage_bytes()`; restore
`storage_write_bytes > snapshot.storage_bytes()` and
`storage_write_bytes == snapshot.byte_length() + 65_536 + 16 + 15`
(`programs/crates/layerx-programs-sandbox/tests/snapshot.rs:384-388`,
`programs/crates/layerx-programs-sandbox/tests/snapshot.rs:576-583`).

Tests:

- `snapshot_destroy_restore_and_continue_preserves_exact_execution_state`
  asserts persist `storage_write_bytes >= snapshot.storage_bytes()`,
  then with write budget `byte_length + 15 - 1` restore is
  `SnapshotRefusal::Meter(BudgetExceeded { StorageWrite, ... })`, and
  with write budget `byte_length + 15 + 65_536 + 16 - 1` it is
  `SnapshotRefusal::Runtime(ExecutionFault::Resource { ... StorageWrite })`.
  Successful restore asserts
  `restore_usage.storage_write_bytes > snapshot.storage_bytes()` and
  `restore_usage.storage_write_bytes == snapshot.byte_length() + 65_536 + 16 + 15`
  (`programs/crates/layerx-programs-sandbox/tests/snapshot.rs:345-583`).
- `aggregate_snapshot_chunks_never_cross_the_lease_namespace_ceiling`
  commits until `TargetBoundExceeded` and asserts live-namespace
  persistent bytes plus snapshot prefix bytes are `<= namespace_bytes`
  (`programs/crates/layerx-programs-sandbox/tests/snapshot.rs:738-802`).
