# LayerX v1 Activity Type Catalogue

The native activity type is a big-endian `u32`, `(module << 16) | ordinal`.
The former byte-pair catalogue is retired: `0x0101` is not SEND.
Native SEND is `0x00010005`. The current native Asset table is section 8.2;
the remaining legacy envelope and module sketches below are historical design
material, not wire definitions. Canonical native definitions live in
`include/layerx/lxp_activity.h`, `include/layerx/lx_asset.h` and `spec.kvx`.

## 1. Scope decision locked for v1

Version 1 covers the **complete agent work lifecycle**, not only economically
meaningful actions. Task commitments, tool-execution attestations, deliveries,
acceptances, rejections and disputes are first-class ordered, authenticated
activities owned by the `service` module. Those lifecycle activities
carry **no direct monetary effect**: the `service` module holds no authority to
move value and MUST NOT emit a balance leg. When a lifecycle event implies value
movement it writes agreement state that becomes a *settlement precondition* for
an `escrow` or `asset` activity, and value moves only when that 402LXP activity
executes. See section 8.6.

## 2. The activity envelope

Every activity is the same envelope; section 8 describes only its `payload`.

```c
struct lxp_activity {
    uint8_t    magic[4];           /* "LXA1" */
    uint16_t   protocol_version;   /* 1 */
    uint32_t   network_id;
    uint16_t   activity_type;      /* section 3 */
    lxp_did_t  actor_did;          /* 32-byte did_ref */
    uint8_t    authority_kind;     /* section 4 */
    lxp_h256_t authority_ref;      /* present iff kind != 0x00 */
    uint64_t   account_sequence;   /* exactly next_sequence[actor] */
    uint64_t   not_before_ms, not_after_ms;   /* inclusive */
    uint8_t    idem_present;
    uint8_t    idempotency_key[16];
    lxp_u128_t fee_limit;
    lxp_h256_t payload_hash;       /* H("LXP1/payload", u16 type || payload) */
    lxp_bytes_t payload;           /* <= LX_MAX_PAYLOAD */
    uint8_t    sig_scheme;         /* 0x01 = Ed25519 */
    lxp_sig_t  signature;          /* over the "LXP1/activity-sign" pre-image */
};
```

The positional encoding, the signing pre-image and `activity_id` are specified
in `wire-format.md`; this document is authoritative only for `activity_type`
values and payload contents. `H(T, B) = SHA-256( u8(len(T)) || T || B )`.

Payload notation: `u8 u16 u32 u64` are fixed-width big-endian unsigned integers;
`u128` and `i128` are 16-byte unsigned and two's-complement signed values under
checked arithmetic; `h256` is a domain-separated 32-byte digest or identifier;
`did` is a `did_ref`, `H("LXP1/did", did_string)`; `acct` is an `account_id` per
`LXP1/account`; `pk32` and `sig64` are an Ed25519 key and signature; `addr20` is
an EVM address; `str<=N` is length-prefixed NFC UTF-8 of at most N bytes;
`blob<=N` is length-prefixed opaque octets; `opt<T>` is presence-flagged;
`vec<T,N>` is a length-prefixed ordered list of at most N elements.

## 3. Type codes and module registry

An activity type is a `uint32_t`: the high 16 bits are the owning module,
the low 16 bits are its ordinal. Zero module and zero ordinal are invalid.

```c
#define LX_TYPE(mod, ord) (((uint32_t)(mod) << 16) | (uint16_t)(ord))
```

The module assignments below are historical; native module identifiers are
specified by `include/layerx/lxp_module.h`.

| Module | ID | Owner directory | Financial authority |
|---|---|---|---|
| `asset` (402LXP) | `0x01` | `src/modules/asset/` | the only balance writer |
| `escrow` | `0x02` | `src/modules/escrow/` | via 402LXP transfer sets |
| `budget` | `0x03` | `src/modules/budget/` | via 402LXP transfer sets |
| `stream` | `0x04` | `src/modules/stream/` | via 402LXP transfer sets |
| `service` | `0x05` | `src/modules/service/` | none |
| `perps` | `0x06` | `src/modules/perps/` | via 402LXP transfer sets |
| `governance` | `0x07` | `src/modules/governance/` | parameters only |
| `bridge` | `0x08` | `src/modules/bridge/` | via 402LXP transfer sets |
| `oracle` | `0x09` | `src/modules/oracle/` | none |
| `identity` | `0x0A` | `src/modules/identity/` | none |

Only `src/modules/asset/lxp_transfer.c` may mutate a balance, through
`lxp_apply_transfer()` and `lxp_apply_transfer_set()`. Every other module builds
a transfer set and calls that kernel; a direct balance write anywhere else is a
conformance failure.

## 4. Authority classes

Authorisation is state-machine logic in `src/protocol/lxp_authority.c`, never
transport middleware. A class is the *semantic* requirement of a type; the wire
`authority_kind` byte is how its proof is referenced.

| Class | Name | `authority_kind` + `authority_ref` | Notes |
|---|---|---|---|
| `A0` | `PRIMARY` | `0x00`, no ref | signature key equals the identity's live primary key |
| `A1` | `SESSION` | `0x01`, `key_id` | expiring, scope-limited, revocable session key |
| `A2` | `CAPABILITY` | `0x02`, `grant_id` of a capability grant | delegated action and spend scopes |
| `A3` | `PAYER_GRANT` | `0x02`, `grant_id` of a payer grant | required for `RECEIVE`; state record is authoritative, not the presented bytes |
| `A4` | `MODULE` | `0x03`, capability id minted by escrow, budget, stream or perps | module-scoped debit rights over one subaccount |
| `A5` | `GOVERNANCE` | `0x00` by a governance member, plus enactment by `0x0703` | parameters, assets, halts, guarantor set |
| `A6` | `ORACLE` | `0x00` by a key in the feed's publisher registry | fail-closed on unknown publisher |
| `A7` | `KEEPER` | `0x00` by any registered account, plus a deterministic precondition | permissionless crank; precondition failure is `LX_ERR_PRECONDITION` |
| `A8` | `BRIDGE_PROOF` | `0x00`, with a finalized Paxeer proof in the payload | proof-carrying, submitter identity irrelevant |

Scope checks are cumulative: an `A1`, `A2` or `A3` authority MUST satisfy the
type allow-list, per-asset cap, per-window cap and expiry of every link in its
delegation chain, capped at `LX_MAX_DELEGATION_DEPTH` (4). A non-existent or
unbound authority is `LX_REJECT`; one that exists and is bound but is expired,
revoked, exhausted or out of scope is `LX_FAIL` and is charged.

## 5. Result codes

`result_code` is a `uint16_t` in the receipt.

| Code | Name | Meaning |
|---|---|---|
| `0x0000` | `LX_OK` | applied |
| `0x0001` | `LX_ERR_DECODE` | envelope or payload not canonical |
| `0x0002` | `LX_ERR_UNKNOWN_TYPE` | type not in this catalogue |
| `0x0003` | `LX_ERR_VERSION` | unsupported `protocol_version` |
| `0x0004` | `LX_ERR_NETWORK` | wrong `network_id` |
| `0x0005` | `LX_ERR_SIG_INVALID` | envelope signature failed |
| `0x0006` | `LX_ERR_ACTOR_UNKNOWN` | `actor_did` not registered |
| `0x0007` | `LX_ERR_SEQUENCE` | `account_sequence` not the expected next value |
| `0x0008` | `LX_ERR_TIMESTAMP_BOUND` | batch timestamp outside `timestamp_bound` |
| `0x0009` | `LX_ERR_IDEMPOTENCY_CONFLICT` | key already produced an economic result |
| `0x000A` | `LX_ERR_AUTHORITY_MISSING` | no authority proof for a required debit |
| `0x000B` | `LX_ERR_AUTHORITY_EXPIRED` | authority past expiry |
| `0x000C` | `LX_ERR_AUTHORITY_SCOPE` | type, asset, amount or window scope exceeded |
| `0x000D` | `LX_ERR_AUTHORITY_REVOKED` | revoked at or before this sequence |
| `0x000E` | `LX_ERR_FEE_LIMIT` | computed fee exceeds `fee_limit` |
| `0x000F` | `LX_ERR_INSUFFICIENT_FEE` | fee payer cannot pay the fee |
| `0x0010` | `LX_ERR_ASSET_UNKNOWN` | asset not in the registry |
| `0x0011` | `LX_ERR_ACCOUNT_UNKNOWN` | account path not materialised |
| `0x0012` | `LX_ERR_INSUFFICIENT_BALANCE` | debit would go negative |
| `0x0013` | `LX_ERR_AMOUNT_ZERO` | amount must be strictly positive |
| `0x0014` | `LX_ERR_OVERFLOW` | checked arithmetic rejected the operation |
| `0x0016` | `LX_ERR_MODULE_DISABLED` | module halted by governance |
| `0x0017` | `LX_ERR_HALTED` | network halted |
| `0x0018` | `LX_ERR_PAYLOAD_HASH` | `payload_hash` mismatch |
| `0x0019` | `LX_ERR_PAYLOAD_SIZE` | payload exceeds the type limit |
| `0x001A` | `LX_ERR_NOT_PERMISSIONED` | actor is not a party to the object |
| `0x001B` | `LX_ERR_PRECONDITION` | keeper precondition not satisfied |
| `0x001C` | `LX_ERR_RATE_LIMIT` | governance rate limit hit |
| `0x001D` | `LX_ERR_OBJECT_NOT_FOUND` | referenced object id does not exist |
| `0x001E` | `LX_ERR_OBJECT_STATE` | object is not in a state that permits this type |
| `0x001F` | `LX_ERR_EXPIRY_RANGE` | expiry or validity window outside the permitted range |
| `0x0020` | `LX_ERR_PARAM_RANGE` | a parameter is outside its declared bounds |
| `0x0021` | `LX_ERR_BPS_RANGE` | a basis-point value exceeds 10000 |

Module-specific codes have the high bit set: `0x8000 | (module << 8) | ordinal`,
written inline in section 8 with their permanent allocation. Each is owned by the
module in its high byte; another module MAY cite an owned code when it means
exactly the same thing, as `perps` cites `oracle`'s `LX_ERR_FEED_UNKNOWN`.

**`LX_REJECT` versus `LX_FAIL`.** Codes `0x0001`–`0x0008`, `0x0018` and `0x0019`
are `LX_REJECT`: the activity never enters the log, so it gets no
`global_sequence`, no receipt, no fee and consumes no account sequence, and a
batch containing one is an invalid batch. Every other code is `LX_FAIL`: the
activity is real history, sequenced, with a durable receipt, a consumed sequence
and a charged fee, all other effects rolled back atomically. A conservation
violation is never a result code — it is `LX_FATAL` and the node stops.

## 6. Fees and transfer notation

Every executed activity charges exactly one fee leg,
`main(fee_payer) -> system:fees : fee_charged`, reserved in phase P8 and omitted
from the transfer columns below because it is universal. The fee payer is the
actor's main account, or the sponsor named in an `A2` sponsorship capability;
`0x0A01` is the only type whose payer is never the actor.

Account paths, encoded as `account_id` per `LXP1/account`: `agent:<did>:main`,
`agent:<did>:budget:<id>`, `agent:<did>:escrow:<id>`, `agent:<did>:stream:<id>`,
`agent:<did>:order:<id>`, `agent:<did>:margin:<position>`,
`system:liquidity:<market>`, `system:funding:<market>`, `system:insurance`,
`system:fees`, `system:paxeer-reserve`, `system:paxeer-withdrawals`. Locked
funds are always real accounts, never hidden balance columns.

Transfer columns use `src -> dst : amount`. Several legs in one cell are legs of
one atomic set with one authorisation context and one receipt: all commit or
none do.

## 7. Universal preconditions

Before any module handler runs, the kernel executes phases P0-P9 of
`state-machine.md`: decode, context admission, signature, actor resolution,
authority resolution, sequence consumption, idempotency, fee reservation. A
module handler therefore never re-checks any of them.

## 8. Catalogue

### 8.1 identity — module `0x0A`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0A01` | `IDENTITY_REGISTER` | `did_string str<=256`, `primary_key pk32`, `pop_signature sig64`, `payout opt<addr20>`, `recovery opt<h256>` | `A0` of the sponsor, plus proof-of-possession by `primary_key` over `H("LXP1/pop", did_ref \|\| primary_key)` | `LX_ERR_DID_TAKEN(0x8A01)`, `LX_ERR_POP_INVALID(0x8A02)`, `LX_ERR_DID_MALFORMED(0x8A03)` | none |
| `0x0A02` | `IDENTITY_ROTATE_PRIMARY` | `new_key pk32`, `pop_signature sig64`, `effective_after_seq u64` | `A0` only | `LX_ERR_POP_INVALID(0x8A02)`, `LX_ERR_SAME_KEY(0x8A04)` | none |
| `0x0A03` | `IDENTITY_ADD_SESSION_KEY` | `session_key pk32`, `expires_at_ms u64`, `type_mask blob<=32`, `spend_cap u128`, `asset opt<h256>` | `A0` | `LX_ERR_SESSION_LIMIT(0x8A05)`, `LX_ERR_EXPIRY_RANGE` | none |
| `0x0A04` | `IDENTITY_REVOKE_SESSION_KEY` | `session_id h256` | `A0` or `A1` revoking itself | `LX_ERR_OBJECT_NOT_FOUND` | none |
| `0x0A05` | `IDENTITY_GRANT_CAPABILITY` | `delegate did`, `type_mask blob<=32`, `asset opt<h256>`, `amount_cap u128`, `window_ms u64`, `window_cap u128`, `expires_at_ms u64`, `subdelegable u8` | `A0`, or `A2` when `subdelegable` and depth < 4 | `LX_ERR_CAP_DEPTH(0x8A07)`, `LX_ERR_CAP_SCOPE_WIDENS(0x8A08)`, `LX_ERR_EXPIRY_RANGE` | none |
| `0x0A06` | `IDENTITY_REVOKE_CAPABILITY` | `capability_id h256`, `cascade u8` | `A0` of the grantor | `LX_ERR_OBJECT_NOT_FOUND` | none |
| `0x0A07` | `IDENTITY_BIND_PAYOUT` | `chain_id u32`, `address addr20`, `binding_proof blob<=128` | `A0` | `LX_ERR_BINDING_PROOF(0x8A09)`, `LX_ERR_BINDING_EXISTS(0x8A0A)` | none |
| `0x0A08` | `IDENTITY_UNBIND_PAYOUT` | `chain_id u32` | `A0` | `LX_ERR_OBJECT_NOT_FOUND` | none |
| `0x0A09` | `IDENTITY_SET_RECOVERY` | `recovery_root h256`, `delay_ms u64` | `A0` | `LX_ERR_EXPIRY_RANGE` | none |
| `0x0A0A` | `IDENTITY_RECOVER` | `new_key pk32`, `recovery_proof blob<=1024`, `pop_signature sig64` | `A7` carrying the recovery proof | `LX_ERR_RECOVERY_PROOF(0x8A0B)`, `LX_ERR_RECOVERY_DELAY(0x8A0C)` | none |

`0x0A02` and `0x0A0A` invalidate every session key and capability granted by the
account at the effective sequence. `0x0A07` binds an EVM payout address for
Paxeer-side settlement only; it grants no LayerX authority.

### 8.2 asset — module `0x01` (402LXP)

All integers are big-endian. Actor means the envelope signer DID. Payloads
are decoded strictly: unsupported versions, truncation and trailing bytes
are refused; these types do not accept opaque event payloads.

| Native type | Ordinal | Name | Payload and transition |
|---|---|---|---|
| `0x00010001` | 1 | REGISTER | `u16=1`, asset id32, salt32, symbol length u8 and 1–16 ASCII bytes, name length u8 and 1–32 UTF-8 bytes, decimals u8 ≤38, cap u128, issuer kind u8, custody reference length u8 and ≤128 bytes. Creates registry metadata; issuer is actor. |
| `0x00010002` | 2 | PAUSE | Legacy direct registry helper; authenticated daemon activity excluded. |
| `0x00010003` | 3 | UNPAUSE | Legacy direct registry helper; authenticated daemon activity excluded. |
| `0x00010004` | 4 | ACCOUNT_OPEN | `u16=1`, asset id32. Opens the actor's per-asset account for a registered, unpaused asset; duplicates refused. |
| `0x00010005` | 5 | SEND | Existing canonical `lxp_send` encoding. Transfers between accounts of the registered asset with all existing authority, sequence and balance checks. |
| `0x00010006` | 6 | RECEIVE | Existing canonical `lxp_receive` encoding. Executes a payer-grant draw with recipient, asset, caps, expiry, purpose and revocation checks. |
| `0x00010007` | 7 | GRANT_ISSUE | Existing canonical payer-grant encoding. Verifies payer authority and persists the grant in module KV. |
| `0x00010008` | 8 | GRANT_REVOKE | `u16=1`, grant id32, revocation sequence u64. Persists revocation at the executing global sequence. |
| `0x00010009` | 9 | WITHDRAW | Existing canonical settlement-owned withdrawal request. Debits the actor through the custody transfer primitive and records the withdrawal nullifier; requires fee parameter encoding 3 for named pricing. |
| `0x0001000a` | 10 | MINT | `u16=1`, asset id32, destination account32, amount u128. Issuer only; positive amount; checked supply and cap; existing account of the asset required. |
| `0x0001000b` | 11 | BURN | `u16=1`, asset id32, source account32, amount u128. Source owner only; positive amount and sufficient balance. |

Native issuer kind 1 requires
`asset_id32 = SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)` and an empty
custody reference. Paxeer custody kind 2 retains its existing asset id.
Cap zero means uncapped. Salt is explicit and persisted, never synthesized.

Public per-asset names are `agent:<DID>:asset:<lowercase hex64 asset_id>`;
`agent:<DID>:main` remains the native-asset account. Native issuance accounts
use `module:asset:value:<hex64 account_id>`. Mint and burn change supply and
balances through conserved kernel transfer sets; neither writes balances
directly. All new activities bind the identity sequence and use the canonical
fee schedule. Supply receipt binding and named fee qualification must be
established separately from payload decoding and transition support.

### 8.3 escrow — module `0x02`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0201` | `ESCROW_OPEN` | `escrow_id h256`, `beneficiary did`, `arbiter opt<did>`, `asset h256`, `amount u128`, `deadline_ms u64`, `terms_hash h256`, `agreement_id opt<h256>` | `A0`, `A1` or `A2` of the owner | `LX_ERR_ESCROW_EXISTS(0x8201)`, `LX_ERR_DEADLINE_RANGE(0x8202)` | `main(owner) -> escrow(id) : amount` |
| `0x0202` | `ESCROW_FUND` | `escrow_id h256`, `amount u128` | `A0`, `A1` or `A2` of the owner | `LX_ERR_OBJECT_STATE` | `main(owner) -> escrow(id) : amount` |
| `0x0203` | `ESCROW_CAPTURE` | `escrow_id h256`, `amount u128` | `A0` of the owner, or `A4` when the bound agreement is `ACCEPTED`, or `A7` after a binding resolution | `LX_ERR_CAPTURE_EXCEEDS(0x8203)`, `LX_ERR_PRECONDITION` | `escrow(id) -> main(beneficiary) : amount` |
| `0x0204` | `ESCROW_RELEASE` | `escrow_id h256`, `amount u128` | `A0` of the beneficiary, or `A4` on agreement cancellation | `LX_ERR_OBJECT_STATE` | `escrow(id) -> main(owner) : amount` |
| `0x0205` | `ESCROW_EXPIRE` | `escrow_id h256` | `A7`, valid only when `batch_timestamp > deadline_ms` and no dispute is open | `LX_ERR_PRECONDITION` | `escrow(id) -> main(owner) : remaining`, `escrow(id) -> main(keeper) : keeper_reward` |
| `0x0206` | `ESCROW_DISPUTE_OPEN` | `escrow_id h256`, `claim_hash h256`, `evidence_uri str<=512` | `A0` of owner or beneficiary | `LX_ERR_DISPUTE_OPEN(0x8204)`, `LX_ERR_DISPUTE_WINDOW(0x8205)` | none |
| `0x0207` | `ESCROW_DISPUTE_RESOLVE` | `escrow_id h256`, `beneficiary_bps u16`, `rationale_hash h256` | `A0` of the named arbiter, or `A5` | `LX_ERR_NOT_PERMISSIONED`, `LX_ERR_BPS_RANGE` | none — writes the binding split |
| `0x0208` | `ESCROW_SETTLE` | `escrow_id h256` | `A7`, valid only when a binding split exists | `LX_ERR_PRECONDITION` | `escrow(id) -> main(beneficiary) : split_b`, `escrow(id) -> main(owner) : remainder` |

`split_b = floor(balance * beneficiary_bps / 10000)`; the remainder, including
all rounding dust, goes to the owner. Rounding always floors toward the party
who is not the claimant.

### 8.4 budget — module `0x03`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0301` | `BUDGET_CREATE` | `budget_id h256`, `delegate did`, `asset h256`, `amount u128`, `window_ms u64`, `window_cap u128`, `expires_at_ms u64`, `recipient_allow vec<did,32>`, `type_mask blob<=32` | `A0` or `A1` of the owner | `LX_ERR_BUDGET_EXISTS(0x8301)`, `LX_ERR_WINDOW_RANGE(0x8302)` | `main(owner) -> budget(id) : amount` |
| `0x0302` | `BUDGET_FUND` | `budget_id h256`, `amount u128` | `A0` or `A1` of the owner | `LX_ERR_OBJECT_STATE` | `main(owner) -> budget(id) : amount` |
| `0x0303` | `BUDGET_SPEND` | `budget_id h256`, `to acct`, `amount u128`, `context_hash h256` | `A2` held by the delegate, resolving to `A4` over the subaccount | `LX_ERR_WINDOW_CAP(0x8303)`, `LX_ERR_RECIPIENT_DENIED(0x8304)`, `LX_ERR_INSUFFICIENT_BALANCE` | `budget(id) -> to : amount` |
| `0x0304` | `BUDGET_AMEND` | `budget_id h256`, `window_cap u128`, `expires_at_ms u64`, `recipient_allow vec<did,32>` | `A0` of the owner | `LX_ERR_OBJECT_STATE` | none |
| `0x0305` | `BUDGET_CLOSE` | `budget_id h256` | `A0` of the owner, or `A7` after expiry | `LX_ERR_PRECONDITION` | `budget(id) -> main(owner) : remaining` |

Recurring windows are computed lazily at spend time:
`window_index = (batch_timestamp - created_at) / window_ms`. There is no timer
activity and no background job; advancing the index resets the spent counter
inside the same transition.

### 8.5 stream — module `0x04`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0401` | `STREAM_OPEN` | `stream_id h256`, `recipient did`, `asset h256`, `deposit u128`, `mode u8 (0=time,1=metered)`, `rate_per_sec u128`, `rate_per_unit u128`, `start_ms u64`, `end_ms u64` | `A0`, `A1` or `A2` of the payer | `LX_ERR_STREAM_EXISTS(0x8401)`, `LX_ERR_RATE_ZERO(0x8402)`, `LX_ERR_MODE(0x8403)` | `main(payer) -> stream(id) : deposit` |
| `0x0402` | `STREAM_FUND` | `stream_id h256`, `amount u128` | `A0`, `A1` or `A2` of the payer | `LX_ERR_OBJECT_STATE` | `main(payer) -> stream(id) : amount` |
| `0x0403` | `STREAM_METER` | `stream_id h256`, `units u128`, `usage_hash h256`, `period_end_ms u64` | `A0` of the recipient, countersigned by an `A2` metering capability from the payer | `LX_ERR_MODE(0x8403)`, `LX_ERR_METER_REGRESS(0x8404)`, `LX_ERR_UNITS_CAP(0x8405)` | none — records accrual only |
| `0x0404` | `STREAM_SETTLE` | `stream_id h256` | `A0` of either party, or `A7` | `LX_ERR_NOTHING_DUE(0x8406)` | `stream(id) -> main(recipient) : accrued` |
| `0x0405` | `STREAM_PAUSE` | `stream_id h256` | `A0` of the payer | `LX_ERR_OBJECT_STATE` | none — settles accrual to the pause instant first |
| `0x0406` | `STREAM_RESUME` | `stream_id h256` | `A0` of the payer | `LX_ERR_OBJECT_STATE` | none |
| `0x0407` | `STREAM_CLOSE` | `stream_id h256` | `A0` of either party, or `A7` after `end_ms` | `LX_ERR_PRECONDITION` | `stream(id) -> main(recipient) : accrued`, `stream(id) -> main(payer) : remainder` |

Time-mode accrual is `min(subaccount_balance, rate_per_sec * elapsed_seconds)`,
with `elapsed_seconds` derived only from batch timestamps by floor division.
Insolvency stops accrual; it never creates debt.

### 8.6 service — module `0x05`

This module orders and attests the complete agent work lifecycle. **No type here
emits a balance leg other than the universal fee**; each writes agreement state
that other modules read as a settlement precondition.

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0501` | `SERVICE_OFFER` | `offer_id h256`, `asset h256`, `price u128`, `spec_hash h256`, `spec_uri str<=512`, `expires_at_ms u64`, `max_accepts u32`, `escrow_required u8`, `arbiter opt<did>` | `A0`, `A1` or `A2` of the provider | `LX_ERR_OFFER_EXISTS(0x8501)`, `LX_ERR_EXPIRY_RANGE` | none |
| `0x0502` | `SERVICE_OFFER_WITHDRAW` | `offer_id h256` | `A0` of the provider | `LX_ERR_OBJECT_STATE` | none |
| `0x0503` | `SERVICE_ACCEPT` | `agreement_id h256`, `offer_id h256`, `escrow_id opt<h256>`, `terms_hash h256`, `deadline_ms u64` | `A0`, `A1` or `A2` of the buyer | `LX_ERR_OFFER_CLOSED(0x8502)`, `LX_ERR_ESCROW_REQUIRED(0x8503)`, `LX_ERR_ESCROW_MISMATCH(0x8504)` | none |
| `0x0504` | `SERVICE_COMMIT` | `agreement_id h256`, `commitment_hash h256`, `plan_uri str<=512`, `eta_ms u64` | `A0`, `A1` or `A2` of the provider | `LX_ERR_NOT_PERMISSIONED`, `LX_ERR_OBJECT_STATE` | none |
| `0x0505` | `SERVICE_ATTEST_TOOL_EXEC` | `agreement_id h256`, `step_index u32`, `tool_id h256`, `input_hash h256`, `output_hash h256`, `exit_code u32`, `started_ms u64`, `ended_ms u64`, `evidence_uri str<=512` | `A0`, `A1` or `A2` of the provider | `LX_ERR_STEP_REGRESS(0x8505)`, `LX_ERR_STEP_LIMIT(0x8506)` | none |
| `0x0506` | `SERVICE_DELIVER` | `agreement_id h256`, `delivery_index u32`, `artifact_hash h256`, `artifact_uri str<=512`, `manifest_hash h256` | `A0`, `A1` or `A2` of the provider | `LX_ERR_OBJECT_STATE`, `LX_ERR_DELIVERY_LIMIT(0x8507)` | none |
| `0x0507` | `SERVICE_ACCEPT_DELIVERY` | `agreement_id h256`, `delivery_index u32`, `rating opt<u8>` | `A0`, `A1` or `A2` of the buyer, or `A7` after `acceptance_window_ms` elapses with no rejection | `LX_ERR_PRECONDITION`, `LX_ERR_OBJECT_STATE` | none — moves the agreement to `ACCEPTED`, which satisfies the `A4` precondition of `0x0203` |
| `0x0508` | `SERVICE_REJECT_DELIVERY` | `agreement_id h256`, `delivery_index u32`, `reason_code u16`, `reason_hash h256` | `A0`, `A1` or `A2` of the buyer | `LX_ERR_OBJECT_STATE`, `LX_ERR_REJECT_LIMIT(0x8508)` | none |
| `0x0509` | `SERVICE_DISPUTE_OPEN` | `agreement_id h256`, `claim_hash h256`, `evidence_uri str<=512`, `claimed_bps u16` | `A0` of buyer or provider | `LX_ERR_DISPUTE_OPEN(0x8509)`, `LX_ERR_DISPUTE_WINDOW(0x850A)` | none |
| `0x050A` | `SERVICE_DISPUTE_RESOLVE` | `agreement_id h256`, `provider_bps u16`, `rationale_hash h256` | `A0` of the named arbiter, or `A5` | `LX_ERR_NOT_PERMISSIONED`, `LX_ERR_BPS_RANGE` | none — writes the binding split into the bound escrow, settled by `0x0208` |
| `0x050B` | `SERVICE_CANCEL` | `agreement_id h256`, `reason_code u16` | `A0` of either party before `SERVICE_COMMIT`, or `A7` after `deadline_ms` with no delivery | `LX_ERR_PRECONDITION`, `LX_ERR_OBJECT_STATE` | none — enables `0x0204` release to the buyer |

Agreement states: `OPEN -> COMMITTED -> DELIVERED -> ACCEPTED | REJECTED |
DISPUTED -> RESOLVED | CANCELLED`. Transitions outside that graph return
`LX_ERR_OBJECT_STATE`. The protocol orders and attests these claims; it does **not**
verify that a tool ran, that an artifact matches its hash off-chain, or that a
deliverable is fit for purpose. `0x0505` and `0x0506` are timestamped,
non-repudiable assertions by the provider, and their truth is an off-protocol
matter for the buyer, the arbiter or a future proof system.

### 8.7 perps — module `0x06`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0601` | `PERPS_CREATE_MARKET` | `market_id h256`, `symbol str<=32`, `quote_asset h256`, `oracle_feed h256`, `tick u64`, `lot u64`, `max_leverage u16`, `imr_bps u16`, `mmr_bps u16`, `funding_interval_ms u64`, `oi_cap u128` | `A5` | `LX_ERR_MARKET_EXISTS(0x8601)`, `LX_ERR_PARAM_RANGE`, `LX_ERR_FEED_UNKNOWN(0x8908)` | none |
| `0x0602` | `PERPS_PLACE_ORDER` | `order_id h256`, `market_id h256`, `side u8`, `price u128`, `size u128`, `tif u8`, `reduce_only u8`, `margin u128` | `A0`, `A1` or `A2` with trading scope | `LX_ERR_TICK(0x8604)`, `LX_ERR_LOT(0x8605)`, `LX_ERR_MARGIN_INSUFFICIENT(0x8606)`, `LX_ERR_MARKET_HALTED(0x8607)`, `LX_ERR_ORACLE_STALE(0x8608)` | `main(actor) -> order(id) : margin`; on deterministic fill also `order(id) -> margin(position) : matched_margin` |
| `0x0603` | `PERPS_CANCEL_ORDER` | `order_id h256` | `A0`, `A1` or `A2` of the owner | `LX_ERR_OBJECT_NOT_FOUND`, `LX_ERR_OBJECT_STATE` | `order(id) -> main(owner) : unfilled_margin` |
| `0x0604` | `PERPS_OPEN` | `position_id h256`, `market_id h256`, `side u8`, `size u128`, `margin u128`, `max_slippage_bps u16` | `A0`, `A1` or `A2` with trading scope | `LX_ERR_SLIPPAGE(0x8609)`, `LX_ERR_OI_CAP(0x860A)`, `LX_ERR_LEVERAGE(0x860B)`, `LX_ERR_ORACLE_STALE(0x8608)` | `main(actor) -> margin(position) : margin`; `margin(position) -> system:fees : taker_fee` |
| `0x0605` | `PERPS_CLOSE` | `position_id h256`, `size u128`, `max_slippage_bps u16` | `A0`, `A1` or `A2` of the owner | `LX_ERR_SLIPPAGE(0x8609)`, `LX_ERR_OBJECT_STATE` | loss: `margin(position) -> system:liquidity:<market>`; profit: `system:liquidity:<market> -> margin(position)`; then `margin(position) -> system:fees : taker_fee`, `margin(position) -> main(owner) : released` |
| `0x0606` | `PERPS_ADJUST_MARGIN` | `position_id h256`, `delta i128` | `A0`, `A1` or `A2` of the owner | `LX_ERR_MMR_BREACH(0x860C)` | add: `main(owner) -> margin(position)`; remove: `margin(position) -> main(owner)` |
| `0x0607` | `PERPS_FUND` | `market_id h256`, `period_index u64` | `A7`, valid only when `batch_timestamp >= last_funding + funding_interval_ms` | `LX_ERR_PRECONDITION`, `LX_ERR_ORACLE_STALE(0x8608)` | one set: each paying position `margin(p) -> system:funding:<market>`, each receiving position `system:funding:<market> -> margin(p)`, rounding dust `system:funding:<market> -> system:insurance` |
| `0x0608` | `PERPS_LIQUIDATE` | `position_id h256`, `max_size u128` | `A7`, valid only when equity < maintenance margin at the oracle mark | `LX_ERR_NOT_LIQUIDATABLE(0x860D)`, `LX_ERR_ORACLE_STALE(0x8608)` | one set: `margin(position) -> system:liquidity:<market> : loss`; `margin(position) -> main(liquidator) : liquidation_fee`; `margin(position) -> system:insurance : insurance_cut`; `system:insurance -> system:liquidity:<market> : deficit`; `margin(position) -> main(owner) : remainder` |
| `0x0609` | `PERPS_SET_MARKET_STATUS` | `market_id h256`, `status u8 (0=active,1=reduce_only,2=halted)` | `A5` | `LX_ERR_PARAM_RANGE` | none |

All perps arithmetic is integer only, on `u128` with `u256` intermediates and
floor rounding that always rounds against the position and toward the liquidity
pool. Every price is the aggregated oracle value for the market feed; if it is
staler than `max_staleness_ms` the module fails closed with `LX_ERR_ORACLE_STALE`.
Funding never mints: `system:funding:<market>` MUST hold zero at the end of
every `0x0607` transfer set.

### 8.8 oracle — module `0x09`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0901` | `ORACLE_OBSERVE` | `feed_id h256`, `observed_at_ms u64`, `value u128`, `decimals u8`, `confidence u32`, `source_hash h256`, `publisher_sig sig64` | `A6` | `LX_ERR_PUBLISHER_UNKNOWN(0x8901)`, `LX_ERR_OBS_REGRESS(0x8902)`, `LX_ERR_OBS_FUTURE(0x8903)`, `LX_ERR_CONFIDENCE(0x8904)`, `LX_ERR_DEVIATION(0x8905)` | none |
| `0x0902` | `ORACLE_REGISTER_FEED` | `feed_id h256`, `symbol str<=32`, `decimals u8`, `max_staleness_ms u64`, `max_deviation_bps u16`, `quorum u8`, `aggregation u8 (0=median)` | `A5` | `LX_ERR_FEED_EXISTS(0x8906)`, `LX_ERR_PARAM_RANGE` | none |
| `0x0903` | `ORACLE_SET_PUBLISHERS` | `feed_id h256`, `publishers vec<{did, pk32},16>` | `A5` | `LX_ERR_FEED_UNKNOWN(0x8908)`, `LX_ERR_QUORUM_RANGE(0x8907)` | none |

An accepted observation becomes replayable history: its exact payload bytes are
committed under the batch `oracle_root` and replayed verbatim. The aggregated
feed value is the median of the newest observation per publisher inside the
staleness window, defined only when at least `quorum` publishers are fresh;
otherwise the feed is `UNDEFINED` and every consumer fails closed. Oracle data
never alters a balance by itself.

### 8.9 governance — module `0x07`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0701` | `GOV_PROPOSE` | `proposal_id h256`, `kind u16`, `params blob<=4096`, `voting_ends_ms u64`, `rationale_uri str<=512` | `A0` of a governance member | `LX_ERR_NOT_MEMBER(0x8701)`, `LX_ERR_KIND(0x8702)` | none |
| `0x0702` | `GOV_VOTE` | `proposal_id h256`, `vote u8` | `A0` of a governance member | `LX_ERR_NOT_MEMBER(0x8701)`, `LX_ERR_DOUBLE_VOTE(0x8703)`, `LX_ERR_VOTING_CLOSED(0x8704)` | none |
| `0x0703` | `GOV_ENACT` | `proposal_id h256` | `A7`, valid only when the threshold is met and the timelock has elapsed | `LX_ERR_PRECONDITION`, `LX_ERR_TIMELOCK(0x8705)` | none |
| `0x0704` | `GOV_EMERGENCY_HALT` | `scope u8 (0=network,1=module)`, `module_id u8`, `reason_hash h256` | `A5` emergency quorum | `LX_ERR_NOT_MEMBER(0x8701)` | none |
| `0x0705` | `GOV_EMERGENCY_RESUME` | `scope u8`, `module_id u8` | `A5` full quorum, timelocked | `LX_ERR_TIMELOCK(0x8705)` | none |
| `0x0706` | `GOV_REGISTER_ASSET` | `asset_id h256`, `symbol str<=16`, `decimals u8`, `paxeer_token addr20`, `withdraw_rate_limit u128` | `A5` | `LX_ERR_ASSET_EXISTS(0x8706)`, `LX_ERR_PARAM_RANGE` | none |
| `0x0707` | `GOV_SET_GUARANTOR_SET` | `epoch u64`, `members vec<{did, secp_pubkey blob<=33>, bond u128},64>`, `threshold u8` | `A5` | `LX_ERR_THRESHOLD_RANGE(0x8707)`, `LX_ERR_SET_SIZE(0x8708)` | none |
| `0x0708` | `GOV_SET_TRANSITION_VERSION` | `activate_at_batch u64`, `version u16`, `code_hash h256` | `A5`, timelocked | `LX_ERR_TIMELOCK(0x8705)`, `LX_ERR_VERSION` | none |

`0x0707` is consensus-critical in two places: the LayerX state machine and the
Paxeer guarantor registry. It takes effect on LayerX only at the named epoch
boundary and only after the identical set is registered on Paxeer; divergence
between the registries halts checkpoint acceptance.

### 8.10 bridge — module `0x08`

| Code | Name | Payload | Authority | Module result codes | 402LXP transfers |
|---|---|---|---|---|---|
| `0x0801` | `BRIDGE_DEPOSIT_PROOF` | `deposit_id h256`, `paxeer_block u64`, `paxeer_tx h256`, `inclusion_proof blob<=4096`, `beneficiary did`, `asset h256`, `amount u128` | `A8` | `LX_ERR_PROOF_INVALID(0x8801)`, `LX_ERR_DEPOSIT_REPLAY(0x8802)`, `LX_ERR_NOT_FINALIZED(0x8803)`, `LX_ERR_ASSET_UNKNOWN` | `system:paxeer-reserve -> main(beneficiary) : amount` |
| `0x0802` | `BRIDGE_WITHDRAW_REQUEST` | `withdrawal_id h256`, `asset h256`, `amount u128`, `payout_chain u32`, `payout_address addr20` | `A0` or `A1` of the owner | `LX_ERR_NO_BINDING(0x8804)`, `LX_ERR_RATE_LIMIT`, `LX_ERR_INSUFFICIENT_BALANCE` | `main(owner) -> system:paxeer-withdrawals : amount` |
| `0x0803` | `BRIDGE_WITHDRAW_CANCEL` | `withdrawal_id h256` | `A0` of the owner, only before the withdrawal's checkpoint is settled | `LX_ERR_OBJECT_STATE` | `system:paxeer-withdrawals -> main(owner) : amount` |
| `0x0804` | `BRIDGE_WITHDRAW_CLAIMED` | `withdrawal_id h256`, `nullifier h256`, `paxeer_tx h256`, `inclusion_proof blob<=4096` | `A8` | `LX_ERR_PROOF_INVALID(0x8801)`, `LX_ERR_NULLIFIER_SPENT(0x8805)` | none — value already left LayerX; marks the nullifier spent |
| `0x0805` | `BRIDGE_EXIT_ANNOUNCE` | `exit_id h256`, `asset h256`, `amount u128`, `payout_address addr20` | `A0` of the owner | `LX_ERR_EXIT_MODE(0x8806)`, `LX_ERR_INSUFFICIENT_BALANCE` | `main(owner) -> system:paxeer-withdrawals : amount` |

A deposit credits an agent only from `system:paxeer-reserve` and only against a
finalized Paxeer proof; LayerX never mints. A withdrawal debits the agent
immediately into `system:paxeer-withdrawals` and is payable on Paxeer only once
the covering checkpoint settles and its challenge window elapses. The nullifier
in `0x0804` is the single anti-double-pay record, mirrored by the contract.

## 9. Global invariants

1. Every monetary mutation runs through `lxp_apply_transfer[_set]()`.
2. Every debit carries explicit authority from section 4.
3. `ASSET_RECEIVE` requires a payer grant.
4. No account may become negative; v1 has no debt representation.
5. Every transfer set satisfies `sum(debits) == sum(credits)` per asset.
6. Transfer sets are all-or-nothing.
7. Every successful transfer has exactly one durable receipt.
8. Every idempotency key yields at most one economic result.
9. Every account sequence is consumed exactly once, failures included.
10. No module outside `src/modules/asset/` writes a balance; oracle data never
    alters one directly.
11. Deposits require finalized Paxeer proof; withdrawals cannot pay twice.
12. Replaying the same history yields identical balances and roots.
13. Service-module activities never move value.

## 10. Conformance

An implementation MUST ship, under `tests/vectors/activity/`, at least one
accepted and one rejected canonical vector per code above, each pinning the
envelope bytes, the `result_code`, the emitted transfer legs in order and the
resulting state root. Two implementations agree only if every vector matches
byte for byte. Fuzz targets in `fuzz/` MUST cover every payload shape here.
