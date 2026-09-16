# Stream module

Module ID `4` (`LXP_MODULE_STREAM`). Activity types are `0x0004xxxx`
(`include/layerx/lx_stream.h`). Streams pay continuously by time or by a
signed meter. Draws still compile to `402LXP` legs from
`agent:<did>:stream:<id>`.

Modes: `LX_STREAM_MODE_TIME = 1`, `LX_STREAM_MODE_METERED = 2`. At most
`LX_STREAM_MAX_METER_AUTHORITIES` (8) meter keys. Payload version is 1
(`bytes[0]=0`, `bytes[1]=1`).

Sources: `include/layerx/lx_stream.h`, `src/modules/stream/`,
`spec/layerx-protocol/spec.kvx` requirement 4.

---

## Activities

| Constant | Value | Decoder | Bytes |
| --- | --- | --- | --- |
| `LX_STREAM_OPEN` | `0x00040001` | `lx_stream_open_decode` | 204 + 32×authorities |
| `LX_STREAM_TOP_UP` | `0x00040002` | `lx_stream_amount_decode` | 50 |
| `LX_STREAM_METER` | `0x00040003` | `lx_stream_meter_decode` | 138 |
| `LX_STREAM_SETTLE` | `0x00040004` | `lx_stream_keyed_decode` | 66 |
| `LX_STREAM_PAUSE` | `0x00040005` | `lx_stream_id_decode` | 34 |
| `LX_STREAM_RESUME` | `0x00040006` | `lx_stream_id_decode` | 34 |
| `LX_STREAM_CLOSE` | `0x00040007` | `lx_stream_keyed_decode` | 66 |

Ordinal `0x00040008` is not declared in `lx_stream.h` and is not registered.

---

## OPEN

After the version prefix (`src/modules/stream/lx_stream_payload.c`):

```text
stream_id32 || stream_account32 || recipient32 || asset_id32
|| mode:u8 || rate:u128 || rate_unit:u64 || start_timestamp:u64
|| end_timestamp:u64 || total_cap:u128 || initial_funding:u128
|| meter_authority_count:u8 || authority_key32 × count
```

Metered mode requires a non-empty, strictly increasing authority list. Time
mode requires count 0. Zero ids, zero rate, zero funding, or
`stream_account == recipient` are `LXP_ERR_NON_CANONICAL`.

---

## TOP_UP

```text
stream_id32 || amount:u128
```

---

## METER

```text
stream_id32 || cumulative_reading:u64 || authority_key32 || signature64
```

Refusals include `LXP_ERR_UNAUTHORIZED_METER`, `LXP_ERR_METER_REGRESSION`,
`LXP_ERR_ACCRUAL_OVERFLOW`, `LXP_ERR_NON_MONOTONIC_TIME`.

---

## SETTLE / CLOSE

```text
stream_id32 || idempotency_key32
```

---

## PAUSE / RESUME

```text
stream_id32
```

---

## Shared refusals

`LXP_ERR_VERSION_UNSUPPORTED`, `LXP_ERR_NON_CANONICAL`,
`LXP_ERR_TRAILING_BYTES`, `LXP_ERR_STREAM_CLOSED`,
`LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_SEQUENCE_REUSED`,
`LXP_ERR_CONTEXT_MISMATCH`, `LXP_ERR_ASSET_MISMATCH`,
`LXP_ERR_ASSET_PAUSED`.

The host binds only published transfer-asset states
(`lx_stream_runtime`). A stream may fund, draw, or refund only those assets.

---

## Start here

- [Modules](Modules.md)
- [Fees](Fees.md)
