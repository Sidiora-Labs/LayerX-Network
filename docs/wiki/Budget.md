# Budget module

Module ID `3` (`LXP_MODULE_BUDGET`). Activity types are `0x0003xxxx`
(`include/layerx/lx_budget.h`). A budget is a funded ceiling, not a second
wallet. Delegation is a grant over that ceiling.

Rollover policies: `LX_BUDGET_ROLLOVER_NONE = 1`,
`LX_BUDGET_ROLLOVER_CAPPED = 2`. At most `LX_BUDGET_MAX_DELEGATES` (16)
delegates. Store capacity is 128 records.

Protocol-3 batch maintenance rolls due periods using the batch timestamp.

Sources: `include/layerx/lx_budget.h`, `src/modules/budget/`,
`spec/layerx-protocol/spec.kvx` requirement 4.

---

## Activities

| Constant | Value | Encoding |
| --- | --- | --- |
| `LX_BUDGET_CREATE` | `0x00030001` | v1 211 bytes or v2 251 bytes |
| `LX_BUDGET_FUND` | `0x00030002` | v1 50 bytes or v2 58 bytes |
| `LX_BUDGET_AMEND` | `0x00030003` | v1 75 bytes |
| `LX_BUDGET_DELEGATE_ADD` | `0x00030004` | v1 66 bytes |
| `LX_BUDGET_DELEGATE_REMOVE` | `0x00030005` | v1 66 bytes |
| `LX_BUDGET_SPEND` | `0x00030006` | v1 82 bytes |
| `LX_BUDGET_CLOSE` | `0x00030007` | v1 42 bytes |

Every payload starts `version:u16` with `bytes[0]=0` and `bytes[1]` the
encoding version (`src/modules/budget/lx_budget_codec.c`). Decode refusals
are `LXP_ERR_NON_CANONICAL`, `LXP_ERR_INVALID_AMOUNT`,
`LXP_ERR_VERSION_UNSUPPORTED`.

---

## CREATE

After the version prefix:

```text
budget_id32 || budget_account32 || asset_id32 || purpose_hash32
|| per_period_limit:u128 || carry_cap:u128 || amount:u128
|| period_length:u64 || period_start:u64 || expiry:u64
|| revocation_sequence:u64 || rollover_policy:u8
```

Version 2 appends `source_account32 || source_sequence:u64`.

---

## FUND

```text
budget_id32 || amount:u128
```

Version 2 appends `source_sequence:u64`.

---

## AMEND

```text
budget_id32 || per_period_limit:u128 || carry_cap:u128
|| expiry:u64 || rollover_policy:u8
```

---

## DELEGATE_ADD / DELEGATE_REMOVE

```text
budget_id32 || delegate32
```

---

## SPEND

```text
budget_id32 || recipient32 || amount:u128
```

---

## CLOSE

```text
budget_id32 || revocation_sequence:u64
```

---

## Refusals

Execute and validate refusals include `LXP_ERR_BUDGET_REVOKED`,
`LXP_ERR_UNKNOWN_FIELD` (closed or missing), `LXP_ERR_UNAUTHORIZED_DEBIT`,
`LXP_ERR_BUDGET_PERIOD_CAP`, `LXP_ERR_BUDGET_ALLOWANCE_EXCEEDED`,
`LXP_ERR_INSUFFICIENT_BUDGET_FUNDS`, `LXP_ERR_UNAUTHORIZED_DELEGATE`,
`LXP_ERR_STALE_REVOCATION`, `LXP_ERR_SEQUENCE_REUSED`, `LXP_ERR_SEQUENCE_GAP`,
`LXP_ERR_EXPIRED`, `LXP_ERR_CONTEXT_MISMATCH`.

Agent-daemon budget objects are a different surface: a daemon-enforced limit
is not a protocol budget. See [Agent API](AgentApi.md).

---

## Start here

- [Modules](Modules.md)
- [Agent API](AgentApi.md)
- [Fees](Fees.md)
