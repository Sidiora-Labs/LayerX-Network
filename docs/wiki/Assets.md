# Assets and tokens

The Asset module is the balance authority for LayerX payments. Amounts are
unsigned integer counts of the smallest unit; `decimals` is display metadata.
The native Asset activity, record-v3, genesis, account-enumeration, and fee
surfaces described here are on the testnet branch.

See also [Payments developer path](PaymentsQuickstart.md),
[Public JSON-RPC](PublicRpc.md), and [Programs](Programs.md).

## Account names and identifiers

Named accounts use:

```text
account_id32 = SHA-256("LX:ACCOUNT:v1" || u32_be(name_length) || name_bytes)
```

All hexadecimal text in account names is lowercase.

| Purpose | Canonical name or identifier |
| --- | --- |
| Native-asset account for a DID | `agent:<DID>:main` |
| Account for one Asset | `agent:<DID>:asset:<asset_id_hex64>` |
| Native Asset id | `SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)` |
| Paxeer-custody Asset id | Supplied custody Asset id; it is not re-derived |

The issuance-account identifier is preserved from the named-account hash of
`asset:<asset_id_hex64>:issuance`. Its current stored name is
`module:asset:value:<issuance_account_id_hex64>`. The migration keeps the
identifier stable while retiring the old name.

## Activity ordinals and payloads

Asset is module `1`. Every integer below is big-endian.

| Ordinal | Operation | Canonical payload |
| ---: | --- | --- |
| `1` | register | `version:u16=1 || asset_id32 || salt32 || symbol_len:u8 || symbol || name_len:u8 || name || decimals:u8 || supply_cap:u128 || issuer_kind:u8 || custody_ref_len:u8 || custody_ref` |
| `2` | pause | Existing pause payload; not admitted by public `lx_sendActivity` |
| `3` | unpause | Existing unpause payload; not admitted by public `lx_sendActivity` |
| `4` | account open | `version:u16=1 || asset_id32` (34 bytes) |
| `5` | send | Canonical `lxp_send` payload, tag `0x5301` |
| `6` | receive | Canonical ten-field `lxp_receive` payload, tag `0x5201` (733 bytes) |
| `7` | grant issue | Canonical payer grant (346 bytes) |
| `8` | grant revoke | `version:u16=1 || grant_id32 || revocation_sequence:u64` (42 bytes) |
| `9` | reserved | Refused; no public payload |
| `10` | mint | `version:u16=1 || asset_id32 || to_account32 || amount:u128` (82 bytes) |
| `11` | burn | `version:u16=1 || asset_id32 || from_account32 || amount:u128` (82 bytes) |

The authenticated public RPC admits ordinals `1`, `4`, `5`, `6`, `7`, `8`,
`10`, and `11`. Payload decoding alone is not execution: execution also checks
the actor, authorization, sequence, registered Asset, pause state, account
ownership, balances, and supply invariants.

### Register

The register payload has these bounds:

- `symbol` is 1–16 ASCII bytes.
- `name` is 1–32 bytes of valid UTF-8.
- `decimals` is at most `38`.
- `supply_cap` is a `u128`; zero means uncapped.
- `issuer_kind` is `1` for native or `2` for Paxeer custody.
- `custody_ref` is at most 128 bytes. Native Assets require an empty custody
  reference and the derived Asset id shown above. Custody Assets require the
  supplied Asset id.

Registration creates a version-3 Asset record and its issuance account.
Duplicate Assets are refused.

### Open, mint, and burn

Account open creates the actor's per-Asset account and refuses an unknown,
paused, or already-open Asset account.

Mint requires the actor to be the issuer, a positive amount, a destination
account for that Asset, and sufficient units in the issuance account. A
nonzero supply cap is enforced. Burn requires a positive amount, an
actor-owned source account for the Asset, and sufficient balance. Mint moves
units from issuance to the destination; burn returns units to issuance.
`total_units` is checked against issuance before every update, and the supply
before/after values are bound into the transition context.

## Receive and payer grants

An ordinal-6 receive is exactly 733 bytes:

```text
0x5201 || field_count:u16=10
|| from32 || to32 || asset32 || amount:u128
|| grant_id32 || receiver_sequence:u64
|| idempotency_key32 || context_hash32
|| receiver_authorization
|| payer_grant
```

The receiver authorization is:

```text
kind:u8 || controller32 || public_key32 || signature64
|| signed_context_hash32 || network_id:u32 || protocol_version:u16
```

The embedded payer grant is exactly 346 bytes:

```text
grant_id32 || from32 || recipient32 || asset32
|| per_draw_maximum:u128 || allowance:u128 || recurring:u8
|| window_length:u64 || expiration:u64 || purpose_hash32
|| has_reference:u8 || reference_hash32 || revocation_sequence:u64
|| public_key32 || signature64
```

The grant authorization preimage begins `LXP:GRANT:v1`; the receiver
authorization preimage begins `LXP:RECEIVE:v1`. The receive must bind the same
grant id, payer, recipient, Asset, purpose, network, and signed context as its
grant and envelope. See [x402 transport](X402Transport.md) for metered draws and
subscription renewals.

## Asset record v3

The persisted canonical record is variable length:

```text
version:u16=3
|| asset_id32
|| symbol_len:u8 || symbol
|| decimals:u8
|| custody_kind:u8
|| custody_ref_len:u16 || custody_ref
|| paused:u8
|| name_len:u8 || name
|| supply_cap:u128
|| issuer_did32
|| issuer_kind:u8
|| total_units:u128
|| salt32
```

Version 2 can be migrated to version 3 by supplying the missing salt and a
non-empty salt-source label:

```sh
layerx-genesis-build --migrate-asset-v2 INPUT SALT_FILE SALT_SOURCE OUTPUT_DIR
```

The migration validates a native derived id and writes `asset-v3.bin` plus
`salt-source.txt`; it does not guess the missing salt.

## LXGB v2 genesis metadata

An `LXGB` version-2 genesis body retains the fixed protocol, network,
timestamp, parameter, guarantor, Asset-id, Programs metering, and Programs fee
fields. It then appends:

```text
asset_record_count:u16
repeat asset_record_count times:
  asset_record_length:u16 || asset_record_v3
fee_schedule_length:u16 || fee_schedule_v2
```

The record count is 1–64, records are keyed by Asset id, every genesis record
has `total_units == 0`, and the requested genesis Asset must be present.
Genesis records use custody issuer kind `2`; native issuance is performed by
an authenticated register activity. Version 1 remains readable for old
artifacts but carries no Asset-record or fee-schedule metadata.

## Named fee schedule

The canonical Asset fee schedule is version 2 and exactly 215 bytes:

```text
version:u16=2
|| base_fee:u128
|| per_activity_type_unit:u128
|| per_encoded_byte:u128
|| per_execution_unit:u128
|| per_storage_unit:u128
|| multiplier_basis_points:u32
|| asset_price_count:u8=8
|| eight asset prices:u128
```

The eight prices are ordered and named:

1. `fee.asset.register` — ordinal `1`
2. `fee.asset.account_open` — ordinal `4`
3. `fee.asset.send` — ordinal `5`
4. `fee.asset.receive` — ordinal `6`
5. `fee.asset.grant_issue` — ordinal `7`
6. `fee.asset.grant_revoke` — ordinal `8`
7. `fee.asset.mint` — ordinal `10`
8. `fee.asset.burn` — ordinal `11`

Fee estimation uses the committed schedule and canonical activity length. An
unsupported Asset ordinal or a schedule requiring unavailable execution or
storage units fails closed.

## Authenticated public reads

The testnet branch's LNI minor-5 read contract provides:

- `AssetReadRequest` version 1: kind `1` lists the complete Asset registry;
  kind `2` gets one nonzero Asset id. The list is bounded to 64 records and is
  sorted by Asset id.
- `AssetReadResponse` version 1: observed sequence, committed state root, count,
  and length-prefixed version-3 records.
- DID account enumeration: the complete bounded account list at the committed
  snapshot, including canonical values and native proof material.
- `FeeEstimateRequest` version 1: activity type, canonical byte count,
  execution units, and storage units. The response carries observed sequence,
  state root, parameter version, decimal fee, and the canonical schedule.

The gateway exposes these through `lx_listAssets`, `lx_getAsset`,
`lx_getBalances`, and `lx_estimateFee`. Their
`verification=authenticated_committed_snapshot` label means an authenticated
same-process committed snapshot; it is not an independent Merkle proof or a
finality claim.

[Home](Home.md)
