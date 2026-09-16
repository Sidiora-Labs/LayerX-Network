# Asset module

Module ID `1` (`LXP_MODULE_ASSET`). Activity types are `0x0001xxxx`
(`include/layerx/lx_asset.h`). The module emits `402LXP` transfer sets; it
does not call `set_balance`.

The registry holds at most `LX_ASSET_REGISTRY_CAPACITY` (64) records
(`include/layerx/lx_asset.h`). Integers on the wire are big-endian. Trailing
bytes are an error.

Account names, issuance, and the named fee schedule are on
[Assets and tokens](Assets.md). Fees are on [Fees](Fees.md).

Sources: `include/layerx/lx_asset.h`, `src/modules/asset/`,
`spec/layerx-protocol/spec.kvx` requirement 14.

---

## Activities

| Constant | Value | Ordinal |
| --- | --- | ---: |
| `LX_ASSET_REGISTER` | `0x00010001` | 1 |
| `LX_ASSET_PAUSE` | `0x00010002` | 2 |
| `LX_ASSET_UNPAUSE` | `0x00010003` | 3 |
| `LX_ASSET_ACCOUNT_OPEN` | `0x00010004` | 4 |
| `LX_ASSET_SEND` | `0x00010005` | 5 |
| `LX_ASSET_RECEIVE` | `0x00010006` | 6 |
| `LX_ASSET_GRANT_ISSUE` | `0x00010007` | 7 |
| `LX_ASSET_GRANT_REVOKE` | `0x00010008` | 8 |
| `LX_ASSET_WITHDRAW` | `0x00010009` | 9 |
| `LX_ASSET_MINT` | `0x0001000a` | 10 |
| `LX_ASSET_BURN` | `0x0001000b` | 11 |

All eleven ordinals are registered on the module iface
(`src/modules/asset/lx_asset_registry.c`). The authenticated public submit
path admits register, pause, unpause, account_open, send, receive,
grant_issue, grant_revoke, mint, and burn. Ordinal 9 is a real kernel
transition and a reserved public operation: that path returns a
reserved-operation refusal.

Unknown ordinals decode as `LXP_ERR_UNKNOWN_ACTIVITY`.

---

## REGISTER

Payload (`lx_asset_register_decode`):

```text
version:u16=1 || asset_id32 || salt32 || symbol_len:u8 || symbol
|| name_len:u8 || name || decimals:u8 || supply_cap:u128
|| issuer_kind:u8 || custody_ref_len:u8 || custody_ref
```

| Field | Constraint |
| --- | --- |
| `symbol` | 1–16 ASCII (`LX_ASSET_SYMBOL_MAX`) |
| `name` | 1–32 UTF-8 (`LX_ASSET_NAME_MAX`) |
| `decimals` | ≤ 38 |
| `issuer_kind` | `1` native, `2` `paxeer_custody` (not `lx_asset_custody_kind`) |
| `custody_ref` | ≤ 128 bytes; empty for native |

Refusals include `LXP_ERR_NON_CANONICAL`, `LXP_ERR_INVALID_AMOUNT`,
`LXP_ERR_ASSET_ALREADY_REGISTERED`, `LXP_ERR_ASSET_MISMATCH` (native id
derivation), `LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_ARENA_EXHAUSTED`.

---

## PAUSE / UNPAUSE

Payload (`asset_pause_decode`): `version:u16=1 || asset_id32` (34 bytes).

Only the recorded issuer may submit. The `paused` flag is written on the
canonical asset record. A paused asset refuses send, mint, burn,
account_open, grant_issue, and withdraw with `LXP_ERR_ASSET_PAUSED`.
Pausing an already paused asset, or unpausing one that is not paused, is
`LXP_ERR_PAUSED_SCOPE`. Wrong issuer is `LXP_ERR_UNAUTHORIZED_DEBIT`.

---

## ACCOUNT_OPEN

Payload: `version:u16=1 || asset_id32` (34 bytes).

Opens the actor's per-asset account. Refusals include `LXP_ERR_ASSET_PAUSED`
and `LXP_ERR_CONTEXT_MISMATCH` when ledger admission facts disagree.

---

## SEND

Canonical `lxp_send` payload (`src/ledger/lxp_send.c`):

```text
tag:u16=0x5301 || fields:u16=10 || from32 || to32 || asset32
|| amount:u128 || sequence:u64 || idempotency_key32 || expires_at:u64
|| context_hash32 || condition_count:u8 || conditions
|| embedded send authorization
```

Sources may be `agent:<did>:main` or `agent:<did>:asset:<hex64>` owned by the
actor. Refusals include `LXP_ERR_MALFORMED_SEND`, `LXP_ERR_UNAUTHORIZED_DEBIT`,
`LXP_ERR_ASSET_PAUSED`, `LXP_ERR_CONTEXT_MISMATCH`, and transfer-set codes
such as `LXP_ERR_INSUFFICIENT_BALANCE` and `LXP_ERR_ZERO_AMOUNT`.

---

## RECEIVE

Canonical `lxp_receive` payload (`src/ledger/lxp_receive.c`): tag `0x5201`,
ten fields, embedded payer grant. Requires a payer grant: one recipient, one
account, caps, purpose, expiry. No wildcards.

Refusals include `LXP_ERR_MALFORMED_RECEIVE`, `LXP_ERR_NO_PAYER_GRANT`,
`LXP_ERR_GRANT_SCOPE_VIOLATION`, `LXP_ERR_GRANT_EXPIRED`,
`LXP_ERR_GRANT_REVOKED`, `LXP_ERR_UNAUTHORIZED_DEBIT`.

---

## GRANT_ISSUE / GRANT_REVOKE

GRANT_ISSUE is a full payer-grant body (`lxp_payer_grant_decode`).
GRANT_REVOKE is `version:u16=1 || grant_id32 || revocation_sequence:u64`
(42 bytes).

Refusals include `LXP_ERR_SEQUENCE_REUSED`, `LXP_ERR_NO_PAYER_GRANT`,
`LXP_ERR_STALE_REVOCATION`, `LXP_ERR_UNAUTHORIZED_DEBIT`,
`LXP_ERR_ASSET_PAUSED`.

---

## WITHDRAW

Kernel payload is 108 bytes with no version prefix
(`src/modules/asset/lx_asset_registry.c`):

```text
asset_id32 || amount:u128 || payout_recipient20 || checkpoint_id32 || fee:u64
```

`fee` must equal the activity `fee_limit` low 64 bits. The actor's
`agent:<did>:main` account for that asset is debited into
`system:paxeer-withdrawals` under `LXP_REASON_WITHDRAWAL`. A second
submission of the same request is `LXP_ERR_WITHDRAWAL_ALREADY_SETTLED`.
A withdraw receipt does not bind supply (`lxp_receipt_validate_supply`
does not describe operation 9).

Refusals also include `LXP_ERR_NON_CANONICAL` (length ≠ 108),
`LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_ASSET_MISMATCH`,
`LXP_ERR_ASSET_PAUSED`, `LXP_ERR_GRANT_SCOPE_VIOLATION`,
`LXP_ERR_CONTEXT_MISMATCH`.

Payout on Paxeer is a [Bridge](Bridge.md) claim against a finalised
checkpoint, not a second Asset ordinal.

---

## MINT / BURN

Payload: `version:u16=1 || asset_id32 || account_id32 || amount:u128`
(82 bytes; amount > 0). Mint `account_id` is the destination; burn
`account_id` is the source.

Refusals include `LXP_ERR_ASSET_PAUSED`, `LXP_ERR_ASSET_MISMATCH`,
`LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_INSUFFICIENT_BALANCE`,
`LXP_ERR_INVALID_AMOUNT`, `LXP_FATAL_SUPPLY_MISMATCH`.

Every executed asset ordinal other than withdraw binds
`total_units_before` / `total_units_after` on the receipt through
`lxp_ctx_bind_asset_supply`.

---

## Start here

- [Modules](Modules.md)
- [Assets and tokens](Assets.md)
- [Fees](Fees.md)
- [Bridge](Bridge.md)
- [Protocol](Protocol.md)
