# Assets and tokens

LayerX amounts are unsigned integer counts of the smallest unit. Decimals
are display metadata only (`spec/layerx-protocol/spec.kvx` on
`lane/pay-native`). `402LXP` is the only balance writer
([Protocol](Protocol.md), [Modules](Modules.md)).

Per-asset agent accounts, native asset-id derivation, register / open /
mint / burn payloads, and the issuance account are implemented
**on the testnet branch** `lane/pay-native`, not merged to
`main`. This page describes the shared wire contract and identifies
source differences between the testnet branches.

Related pages: [Payments developer path](PaymentsQuickstart.md),
[Public JSON-RPC](PublicRpc.md), [Modules](Modules.md), [Custody](Custody.md).

---

## Account names

Integers on the wire are big-endian. `H` is SHA-256.

| Account | Name | Id |
| --- | --- | --- |
| Native-asset agent account | `agent:<DID>:main` | existing `LX:ACCOUNT:v1` rule |
| Per-asset agent account | `agent:<DID>:asset:<lowercase hex64 asset_id>` | same rule |
| Issuance account | `asset:<lowercase hex64 asset_id>:issuance` | `LX:ACCOUNT:v1` named-account hash; module-value account on the testnet branch |

`LX:ACCOUNT:v1` for named (non-module-value) accounts is already on
`main`:

```
account_id32 = SHA-256("LX:ACCOUNT:v1" || u32_be(name_length) || name_bytes)
```

(`src/ledger/lx_account_id.c`). Allowed name bytes are `a-z`, `0-9`,
`.`, `_`, `-`, `:`; empty segments are refused.

On `main`, `lx_account_name_parse` accepts `agent:<DID>:main` and the
budget / escrow / stream / margin forms. The `:asset:` suffix is added
on `lane/pay-native` (`src/ledger/lx_account_id.c` on that branch).
`agent:<DID>:main` remains the native-asset account.

On the testnet branch, register execution stages `asset:<hex64>:issuance`
through `lxp_ctx_asset_issuance_stage` (`src/modules/asset/lx_asset_execution.h`).

---

## Asset identifiers

| Kind | Id |
| --- | --- |
| Natively issued token | `asset_id32 = H("LX:ASSET:v1" \|\| issuer_did_id32 \|\| salt32)` |
| Paxeer-custody asset | existing registered id; not re-derived |

`issuer_did_id32` is the identity id32 of the envelope signer DID
(Actor). Actor is the envelope signer.

`lane/pay-signer-sdk` implements `asset_id(issuer, salt)` as that hash
(`agent/crates/layerx-crypto/src/payments.rs` on that branch).
`lane/pay-native` verifies the same hash during register execution
(`src/modules/asset/lx_asset_execution.h`), after payload decoding.

---

## Asset activity ordinals

Module id `1` (`0x0001xxxx`). Ordinal is the low 16 bits of
`activity_type` (`src/protocol/lxp_activity.c`).

| Ordinal | Type | Payload |
| ---: | --- | --- |
| 1 | register | `version:u16=1 \|\| asset_id32 \|\| salt32 \|\| symbol_len:u8 \|\| symbol(1..16 ASCII) \|\| name_len:u8 \|\| name(1..32 UTF-8) \|\| decimals:u8(<=38) \|\| supply_cap:u128 (0 = uncapped) \|\| issuer_kind:u8 (1 native, 2 paxeer_custody) \|\| custody_ref_len:u8 \|\| custody_ref(<=128)` |
| 2 | pause | existing pause activity |
| 3 | unpause | existing unpause activity |
| 4 | account_open | `version:u16=1 \|\| asset_id32` |
| 5 | send | existing `lxp_send` encoding (`src/ledger/lxp_send.c`, tag `0x5301`) |
| 6 | receive | existing `lxp_receive` encoding (`src/ledger/lxp_receive.c`, tag `0x5201`, 10 fields) |
| 7 | grant_issue | existing payer-grant canonical encoding |
| 8 | grant_revoke | `version:u16=1 \|\| grant_id32 \|\| revocation_sequence:u64` |
| 9 | **RESERVED** | WITHDRAW; no payload defined here. |
| 10 | mint | `version:u16=1 \|\| asset_id32 \|\| to_account32 \|\| amount:u128` |
| 11 | burn | `version:u16=1 \|\| asset_id32 \|\| from_account32 \|\| amount:u128` |

On `main`, `include/layerx/lx_asset.h` defines ordinals 1–8 only
(`LX_ASSET_REGISTER` … `LX_ASSET_GRANT_REVOKE`). Mint and burn constants
`0x0001000a` / `0x0001000b` are **on the testnet branch**
`lane/pay-native`. Ordinal 9 is absent on `main` and rejected on
`lane/pay-native` (`LXP_ERR_UNKNOWN_ACTIVITY`).

---

## Register (ordinal 1)

Issuer = actor.

- Kind `1` (native): `asset_id` must match
  `H("LX:ASSET:v1" || issuer_did_id32 || salt32)`; `custody_ref_len`
  must be `0`.
- Kind `2` (paxeer_custody): custody reference is 0..128 bytes; asset id
  is the existing custody id.
- Duplicates are refused.
- `supply_cap` `0` means uncapped.

On `lane/pay-native`, decode enforces version `1`, symbol length 1..16
with bytes `<= 0x7F`, UTF-8 name 1..32, decimals `<= 38`, issuer_kind
`1` or `2`, and native custody length `0`
(`src/modules/asset/lx_asset_decode.c` on that branch). Decode does not
check the native asset-id hash; execution checks it, refuses duplicates,
saves metadata, and stages the issuance account
(`src/modules/asset/lx_asset_execution.h`).

The helper `lx_asset_register()` on `main` still requires a Paxeer
custody reference and `A-Z0-9` symbols
(`src/modules/asset/lx_asset_registry.c`). That helper is not the
ordinal-1 activity decoder.

---

## Account open (ordinal 4)

Payload is exactly 34 bytes: `version:u16=1 || asset_id32`.

Opens the actor's per-asset account `agent:<DID>:asset:<hex64>`. The
account must not already exist. The asset must be registered and
unpaused.

On the testnet branch `lane/pay-native`, execution loads the asset,
refuses paused assets and stages the account. Helper
`lx_asset_account_open` on `main` still takes a caller-supplied name
(`include/layerx/lx_asset.h`).

---

## Receive and grants (ordinals 6–8)

Ordinal 6 reuses `lxp_receive_encode` / `lxp_receive_decode` on `main`
(`src/ledger/lxp_receive.c`):

```
tag:u16=0x5201 || field_count:u16=10
|| from32 || to32 || asset32 || amount:u128
|| grant_id32 || receiver_sequence:u64 || idempotency_key32 || context_hash32
|| receiver_authorization || payer_grant
```

Receiver authorization is
`kind:u8 || controller32 || public_key32 || signature64 || signed_context_hash32 || network_id:u32 || protocol_version:u16`.
The payer grant is the existing 346-byte grant struct. Grant issue
(ordinal 7) is that same grant encoding. Authorization preimages are
`LXP:RECEIVE:v1` and `LXP:GRANT:v1` (`src/ledger/lxp_receive.c`,
`src/ledger/lxp_grant_store.c`).

`lane/pay-402lxp` documents the concatenated receive as 733 bytes
(`spec/402lxp/protocol.md` on that branch).

Ordinal 8 revoke: `version:u16=1 || grant_id32 || revocation_sequence:u64`
(exactly 42 bytes on `lane/pay-native`).

**On the testnet branch** `lane/pay-signer-sdk`, `Payment::Receive` encodes
an 8-field body (`field_count=8`, grant id only, no embedded
authorization or grant) and `Payment::IssueGrant` encodes a capability
grant under structure header `0x2001`. Those encodings are not the
shared receive / payer-grant wires.

---

## Mint and burn (ordinals 10–11)

Shared payload, 82 bytes:

```
version:u16=1 || asset_id32 || account32 || amount:u128
```

`amount` must be `> 0`.

- **Mint:** actor is the asset issuer; `total_units + amount` must be
  `<= supply_cap` when the cap is nonzero; destination account must
  exist for that asset.
- **Burn:** actor owns `from_account`; balance must cover `amount`.

On the testnet branch (`src/modules/asset/lx_asset_execution.h`), registration
creates `asset:<hex>:issuance` with initial units equal to `supply_cap`,
or `u128` max when uncapped. Mint transfers issuance → destination.
Burn transfers source → issuance. `total_units` is initial issuance
minus current issuance balance. The asset record also stores `total_units`;
execution checks that it equals this derived value before updating it.

On `lane/pay-native`, mint/burn decode rejects zero amount
(`LXP_ERR_INVALID_AMOUNT`). Execution checks issuer/owner authority, asset
matching, pause state, supply bounds and balances, then emits a monetary
transfer and saves the updated total. Receipts on `main` bind
from/to and before/after balances (`include/layerx/lxp_receipt.h`); they
do not carry a `total_units` field.

---

## Fees, sequence, receipts

Every new activity consumes `identity.next_sequence` the same way SEND
does and is charged by the existing fee schedule with type prices for
the new ordinals. A receipt binds both transfer endpoints and their
before/after balances. The shared rule also binds `total_units`; that
field is not on the `main` receipt schema.

---

## LXT-20 program requests

**On the testnet branch** `lane/pay-programs-tokens`, LXT-20 is a guest
calldata codec, not a second ledger:

```
selector:4 || 0x01 || 0x20 || payload_len:u32_be || payload
```

Selector bytes are `4c 58 14` plus method `1..7` (transfer, approve,
transfer_from, balance_of, allowance, total_supply, metadata)
(`programs/sdk/rust/src/lxt20.rs` on that branch). Creating a token is
still Asset register / open / mint, not an LXT-20 request.

---

## Public reads

**On the testnet branch** `lane/pay-public-rpc`, `lx_listAssets` and
`lx_getAsset` are declared on `POST /rpc`. The OpenRPC document states
that asset listing and detail forward to core and return explicit
upstream unavailability until native integration exists
(`platform/hosted/gateway/openrpc.json` on that branch). See
[Public JSON-RPC](PublicRpc.md).

[Home](Home.md)
