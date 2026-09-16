# Fees

Fees are computed from the committed canonical schedule
(`include/layerx/lxp_fee.h`, `src/protocol/lxp_fee.c`). Public estimation
returns the schedule and snapshot that produced the value. It does not
reserve the fee or prove execution.

Normative rules: `spec/layerx-protocol/spec.kvx` requirements 2 (acceptance 8),
14 (encodings 3 and 4), and 26.

---

## Schedule versions

| Version | Encoded size | Extra prices |
| ---: | ---: | --- |
| 1 | 86 bytes | coefficients only |
| 2 | 87 + 16×10 | ten Asset ordinal prices |
| 3 | V2 + 8 | eleven Asset prices; withdraw `u64` slot must have high 64 bits zero |
| 4 | V3 head + 16×7 | seven module prices (escrow through bridge) |

`lxp_fee_params` fields: `base_fee`, `per_activity_type_unit`,
`per_encoded_byte`, `per_execution_unit`, `per_storage_unit`,
`multiplier_basis_points`, `asset_prices[]`, `module_prices[]`.

Governance parameter keys (`src/modules/governance/lxp_fee_params.c`):
`fee.base`, `fee.activity`, `fee.byte`, `fee.exec`, `fee.storage`,
`fee.multiplier_bps`, `fee.encoding`. Committed bytes live at `fee.schedule`
and, for version 4, `fee.module-prices`.

---

## Named Asset prices

Storage order maps to ordinals `{1,2,3,4,5,6,7,8,10,11,9}`:

| Index | Name | Ordinal |
| ---: | --- | ---: |
| 0 | `fee.asset.register` | 1 |
| 1 | `fee.asset.pause` | 2 |
| 2 | `fee.asset.unpause` | 3 |
| 3 | `fee.asset.account_open` | 4 |
| 4 | `fee.asset.send` | 5 |
| 5 | `fee.asset.receive` | 6 |
| 6 | `fee.asset.grant_issue` | 7 |
| 7 | `fee.asset.grant_revoke` | 8 |
| 8 | `fee.asset.mint` | 10 |
| 9 | `fee.asset.burn` | 11 |
| 10 | `fee.asset.withdraw` | 9 (version ≥ 3) |

`lxp_asset_fee_name` reports the version-2 names (ten prices). Version 3 and 4
add withdraw.

---

## Named module prices (version 4)

Indices 0–6 are modules 2–8:

`fee.module.escrow`, `fee.module.budget`, `fee.module.stream`,
`fee.module.service`, `fee.module.perps`, `fee.module.governance`,
`fee.module.bridge`.

Programs is not in this table. A Programs CALL that presents
`exact_program_fee_present` uses `exact_program_fee_units` instead of the
coefficient formula (`lxp_fee_compute`). That path is only legal for module 9
ordinal 3.

---

## Formula

`lxp_fee_compute`:

1. If an exact program fee is present, return those units (CALL only).
2. Start at `base_fee`.
3. Asset module: add the flat `asset_prices` slot for the ordinal. Other
   modules: add `per_activity_type_unit × activity_type`.
4. Version 4: if the module is escrow–bridge, add that module price.
5. Add `per_encoded_byte × canonical_encoded_bytes`,
   `per_execution_unit × execution_units`,
   `per_storage_unit × storage_units`.
6. Apply `lxp_u128_mul_bps_ceil(total, multiplier_basis_points)`.

An unknown Asset ordinal under version ≥ 2 is `LXP_ERR_UNKNOWN_ACTIVITY`.
Unsupported schedule versions are `LXP_ERR_VERSION_UNSUPPORTED`.

---

## Admission and failed activities

`lxp_fee_admission_check` / `lxp_fee_rejection_policy`
(`src/protocol/lxp_fee_policy.c`) and requirement 26:

- Admission failure: no global sequence, no account sequence, no fee, no
  module effects.
- Admitted execution that then fails: sequence is consumed; fee is charged
  up to `fee_limit` (`LXP_ERR_FEE_LIMIT` if computed fee exceeds the limit);
  module effects roll back; a failure receipt is emitted.
- `fee_charged` is never greater than `fee_limit`.
- Spendable fee balance below `fee_limit` is `LXP_ERR_FEE_UNPAYABLE`.

The treasury account is `system:fees`. Charge is a `402LXP` leg with
`LXP_REASON_PROTOCOL_FEE`.

Programs occupancy rent is a separate batch settlement, also as `402LXP`
legs. See [Programs](Programs.md).

LNI `fee_estimate` and `session_fee_state` (minor 1.5) return the committed
schedule projection. They do not reserve.

---

## Start here

- [Protocol](Protocol.md)
- [Assets and tokens](Assets.md)
- [Programs](Programs.md)
- [LNI](LNI.md)
