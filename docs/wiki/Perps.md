# Perps module

Module ID `6` (`LXP_MODULE_PERPS`). Activity types are `0x0006xxxx`
(`include/layerx/lx_perps.h`). Positions, margin, funding, liquidation, and
insurance are module state. Losses and fees are transfer legs, not shadow
balances.

Oracle prices enter as signed `ORACLE_PUSH` activities. Execution does not
dial out (`spec/layerx-protocol/spec.kvx` requirement 4 acceptance 5).

Sources: `include/layerx/lx_perps.h`, `src/modules/perps/`,
`spec/layerx-protocol/spec.kvx` requirement 19.

---

## Activities

| Constant | Value | Bytes |
| --- | --- | --- |
| `LX_PERPS_MARKET_CREATE` | `0x00060001` | 622 (`lx_perps_market`) |
| `LX_PERPS_MARKET_HALT` | `0x00060002` | 33 |
| `LX_PERPS_ORACLE_PUSH` | `0x00060003` | 72 |
| `LX_PERPS_ORDER_PLACE` | `0x00060004` | 129 |
| `LX_PERPS_ORDER_CANCEL` | `0x00060005` | 64 |
| `LX_PERPS_POSITION_OPEN` | `0x00060006` | 145 |
| `LX_PERPS_POSITION_INCREASE` | `0x00060007` | 112 |
| `LX_PERPS_POSITION_CLOSE` | `0x00060008` | 64 |
| `LX_PERPS_FUNDING_TICK` | `0x00060009` | 32 |
| `LX_PERPS_LIQUIDATE` | `0x0006000a` | 96 |
| `LX_PERPS_ADL` | `0x0006000b` | 33–4129 |

Unknown ordinals are `LXP_ERR_UNKNOWN_ACTIVITY`. Malformed commands are
`LXP_ERR_NON_CANONICAL` or `LXP_ERR_LENGTH_LIMIT`.

Sides: `LX_PERPS_SIDE_BUY = 1`, `LX_PERPS_SIDE_SELL = 2`.

---

## MARKET_CREATE

The payload is the canonical 622-byte `lx_perps_market` blob
(`lx_perps_market_decode`): market id, quote asset, administrator, liquidity
/ long-funding / short-funding / insurance account ids, contract/tick/lot
sizes, price scale, margin and liquidation basis-point fields, funding
interval, oracle staleness and price bounds, up to eight permitted oracle
keys, parameter version, and halted flag.

A duplicate market is `LXP_ERR_MARKET_ALREADY_EXISTS`. Parameter bounds
failures are `LXP_ERR_PARAMETER_BOUNDS`.

---

## MARKET_HALT

```text
market_id32 || halted:u8
```

---

## ORACLE_PUSH

Canonical command encoding (`lx_perps_oracle_command_decode`):

```text
market_id32 || observation_sequence:u64 || price:u128
|| observed_at:u64 || source_identifier:u64
```

The full command struct also carries `oracle_public_key32` and `signature64`
for the sign path. Refusals include `LXP_ERR_UNAUTHORIZED_ORACLE`,
`LXP_ERR_ORACLE_SEQUENCE`, `LXP_ERR_ORACLE_BOUNDS`, `LXP_ERR_ORACLE_DEVIATION`,
`LXP_ERR_ORACLE_STALE`, `LXP_ERR_TIMESTAMP_REGRESSION`.

There is no Programs host function named `oracle_read`. See
[Roadmap: beta surface expansion](Roadmap.md).

---

## ORDER_PLACE

```text
market_id32 || order_id32 || owner_account_id32 || side:u8
|| price:u128 || quantity:u128
```

A halted market is `LXP_ERR_MARKET_HALTED`. Insufficient margin is
`LXP_ERR_MARGIN_INSUFFICIENT`. Zero size is `LXP_ERR_ZERO_AMOUNT`.

---

## ORDER_CANCEL

```text
market_id32 || order_id32
```

---

## POSITION_OPEN

```text
market_id32 || position_id32 || margin_account_id32 || side:u8
|| size:u128 || entry_notional:u128 || margin_amount:u128
```

Opening posts margin from the owner main account into the margin account.

---

## POSITION_INCREASE

```text
market_id32 || position_id32 || size_delta:u128
|| notional_delta:u128 || margin_amount:u128
```

---

## POSITION_CLOSE

```text
market_id32 || position_id32
```

---

## FUNDING_TICK

```text
market_id32
```

Uses the sealed batch timestamp, not a wall clock.

---

## LIQUIDATE

```text
market_id32 || position_id32 || liquidator_account_id32
```

---

## ADL

```text
market_id32 || position_count:u8 || position_id32 × count
```

Position ids must be sorted (`LXP_ERR_UNSORTED_SEQUENCE`). Count is bounded
by `LX_PERPS_ADL_CAPACITY` (128).

---

## Shared refusals

`LXP_ERR_MARKET_HALTED`, `LXP_ERR_UNAUTHORIZED_DEBIT`,
`LXP_ERR_SEQUENCE_REUSED`, `LXP_ERR_AGREEMENT_STATE`,
`LXP_ERR_CONSERVATION`, `LXP_ERR_TOO_MANY_LEGS`,
`LXP_ERR_ACCOUNT_NOT_EMPTY`, `LXP_ERR_NOT_YET_VALID`.

---

## Start here

- [Modules](Modules.md)
- [Fees](Fees.md)
- [Roadmap](Roadmap.md)
