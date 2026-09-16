# Service module

Module ID `5` (`LXP_MODULE_SERVICE`). Activity types are `0x0005xxxx`
(`include/layerx/lx_service.h`). Offers, agreements, attestations, delivery,
and disputes are module state. Payment still walks through `402LXP` (typically
an escrow the agreement names). The module must not write balances
(`LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE`).

Payload version is `LX_SERVICE_RECORD_VERSION = 1` (`bytes[0]=0`,
`bytes[1]=1`). At most `LX_SERVICE_MAX_DELIVERABLES` (16) hashes or items.

Event types mirror activity ordinals 1–13; event 14 is default acceptance
applied by batch maintenance.

Sources: `include/layerx/lx_service.h`, `src/modules/service/`,
`spec/layerx-protocol/spec.kvx` requirement 4.

---

## Activities

| Constant | Value | Payload |
| --- | --- | --- |
| `LX_SERVICE_OFFER_PUBLISH` | `0x00050001` | 179 bytes |
| `LX_SERVICE_OFFER_WITHDRAW` | `0x00050002` | 34-byte identifier |
| `LX_SERVICE_AGREEMENT_PROPOSE` | `0x00050003` | 130 bytes |
| `LX_SERVICE_AGREEMENT_ACCEPT` | `0x00050004` | 34-byte identifier |
| `LX_SERVICE_COMMIT_TASK` | `0x00050005` | 146 bytes |
| `LX_SERVICE_COMMIT_ABANDON` | `0x00050006` | 36 bytes |
| `LX_SERVICE_TOOL_EXEC_ATTEST` | `0x00050007` | execution record (variable) |
| `LX_SERVICE_PROGRESS_REPORT` | `0x00050008` | 134 bytes |
| `LX_SERVICE_DELIVER` | `0x00050009` | 67 + 72×items |
| `LX_SERVICE_ACCEPT` | `0x0005000a` | 34-byte identifier |
| `LX_SERVICE_REJECT` | `0x0005000b` | 37 + 32×hashes |
| `LX_SERVICE_DISPUTE_OPEN` | `0x0005000c` | 67 + 32×hashes |
| `LX_SERVICE_DISPUTE_RESOLVE` | `0x0005000d` | 72 bytes |

Unknown ordinals are `LXP_ERR_UNKNOWN_ACTIVITY`. Version mismatches are
`LXP_ERR_VERSION_UNSUPPORTED`. Wrong length is `LXP_ERR_TRAILING_BYTES` or
`LXP_ERR_TRUNCATED`.

---

## OFFER_PUBLISH

After the version prefix (`src/modules/service/lx_service_payload.c`):

```text
offer_id32 || asset_id32 || price:u128 || terms_hash32
|| deliverable_specification_hash32 || delivery_deadline:u64
|| acceptance_window:u64 || dispute_window:u64
|| default_outcome:u8 || offer_expiry:u64
```

`default_outcome` is `1` accept or `2` reject. Zero ids, zero price, or zero
windows are `LXP_ERR_NON_CANONICAL`.

---

## Identifier activities

OFFER_WITHDRAW, AGREEMENT_ACCEPT, and ACCEPT carry `identifier32` after the
version prefix (34 bytes total). Zero identifier is `LXP_ERR_NON_CANONICAL`.

---

## AGREEMENT_PROPOSE

```text
agreement_id32 || offer_id32 || terms_hash32 || escrow_id32
```

---

## COMMIT_TASK

```text
commitment_id32 || agreement_id32 || task_hash32 || escrow_id32
|| deadline:u64 || resource_bound:u64
```

Zero deadline or resource bound is `LXP_ERR_NON_CANONICAL`.

---

## COMMIT_ABANDON

```text
commitment_id32 || abandon_reason:u16
```

---

## TOOL_EXEC_ATTEST

Decoded by `lx_service_execution_decode`. The execution record includes
attestation id, agreement and commitment ids, tool id, input/output
commitment hashes, execution window, resource units, attestor identity,
availability reference, public key, and signature. Verification is
`lx_service_attestor_verify`. Invalid attestations are
`LXP_ERR_INVALID_ATTESTATION`.

---

## PROGRESS_REPORT

```text
report_id32 || commitment_id32 || note_hash32 || availability_reference32
|| progress_bps:u32
```

`progress_bps` must be in `1..=10000` or decode returns
`LXP_ERR_PARAMETER_BOUNDS`. Regression is `LXP_ERR_METER_REGRESSION`.

---

## DELIVER

```text
delivery_id32 || agreement_id32 || count:u8
|| {hash32 || artifact_size:u64 || availability_reference32} × count
```

Count must be 1–16. Zero hashes or sizes are `LXP_ERR_NON_CANONICAL`.
Mismatched deliverables are `LXP_ERR_DELIVERABLE_MISMATCH`. Missing
availability is `LXP_ERR_DA_MISSING`. Past deadline is
`LXP_ERR_DELIVERY_DEADLINE_PASSED`.

---

## REJECT

```text
agreement_id32 || rejection_reason:u16 || count:u8 || contested_hash32 × count
```

---

## DISPUTE_OPEN

```text
dispute_id32 || agreement_id32 || count:u8 || evidence_hash32 × count
```

Unauthorized raisers are `LXP_ERR_UNAUTHORIZED_DISPUTANT`. Closed windows
are `LXP_ERR_DISPUTE_WINDOW_CLOSED`.

---

## DISPUTE_RESOLVE

```text
dispute_id32 || ruling:u16 || provider_basis_points:u32
|| escrow_resolution_id32
```

`provider_basis_points` above 10000, zero ruling, or zero ids is
`LXP_ERR_PARAMETER_BOUNDS`.

---

## Shared execute refusals

`LXP_ERR_OFFER_UNAVAILABLE`, `LXP_ERR_TERMS_MISMATCH`,
`LXP_ERR_AGREEMENT_STATE`, `LXP_ERR_SEQUENCE_REUSED`,
`LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE`.

Protocol-3 batch maintenance applies default acceptance when the acceptance
window ends (`lx_service_acceptance_default`).

---

## Start here

- [Modules](Modules.md)
- [Escrow](Escrow.md)
- [Fees](Fees.md)
