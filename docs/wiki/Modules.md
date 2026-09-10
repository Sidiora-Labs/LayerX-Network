<!--
Draft copy for the GitHub wiki page "Modules".
The wiki has no PR flow, so this file is the reviewable source. After this PR
merges, paste the body below (everything under the first `# Modules`) into the
wiki page. Do not commit this note to the wiki.
-->

# Modules

Eight economic modules (`0x01`–`0x08`) plus Programs (`0x09`) on a kernel that owns identity and authority.

Oracle intake remains an outside adapter, not a module ID. Programs is module ID `9` (`LXP_MODULE_PROGRAMS`); it is the guest-execution module, not a ninth `402LXP` writer. See [Programs](Programs.md).

---

## The registered set (0x01–0x09)

| ID | Module | What it does |
| --- | --- | --- |
| `0x01` | asset | Transfer sets that move value. `402LXP` is the writer. |
| `0x02` | escrow | Money held until the terms are met |
| `0x03` | budget | A hard ceiling on what an agent may spend |
| `0x04` | stream | Paying continuously, by the unit |
| `0x05` | service | Agreeing work, proving it, delivering it |
| `0x06` | perps | Leveraged positions and their margin accounts |
| `0x07` | governance | Changing protocol settings, on a timelock |
| `0x08` | bridge | Custody on Paxeer L1, and withdrawal claims |
| `0x09` | programs | Guest WASM execution. Emits transfer sets; does not write balances. |

Module IDs are stable and never reused. They occupy the high 16 bits of `activity_type` (`include/layerx/lxp_module.h`). An unknown or epoch-disabled module is refused - not best-effort decoded.

Runtime sources live under `src/modules/` (`asset`, `escrow`, `budget`, `stream`, `service`, `perps`, `governance`, `bridge`, `programs`). Genesis registers Programs v4 for every accepted protocol version; Asset v1 is protocol-3-conditional (`src/protocol/lxp_genesis.c:594-598`). Since the monorepo integration, the Paxeer settlement stack these modules checkpoint to lives in the same repository under `paxeer-network/` (EVM chain ID `125`).

---

## One doorway for money

`402LXP` is the sole balance writer. That is the feature, not a caveat.

Modules never call `set_balance`. They emit transfer sets - one or more legs with a single authorization context, a single sequence, and a single receipt. All legs commit, or none do. Per asset, Σ debits = Σ credits.

Locked funds are real accounts, not hidden columns:

```
agent:<did>:main
agent:<did>:asset:<lowercase hex64 asset_id>
agent:<did>:budget:<id>
agent:<did>:escrow:<id>
agent:<did>:stream:<id>
agent:<did>:margin:<position>
module:asset:value:<lowercase hex64 account-id>      (an asset's issuance account)
module:programs:value:<account-id>
system:fees
system:paxeer-reserve
```

Opening a position is a transfer into a margin account. Capturing escrow is a transfer out of an escrow account. Native issuance still compiles to `402LXP` legs against the issuance account; modules do not assign balances. `agent:<did>:main` remains the native-asset account. Per-asset accounts use the existing `LX:ACCOUNT:v1` id rule. An issuance account's id is that rule applied to the string `asset:<lowercase hex64 asset_id>:issuance`, while the name it is stored under is the `module:asset:value:` form above; records still carrying the older string as their name are renamed on load. See `spec/layerx-protocol/spec.kvx` requirement 14.

**One record per (account, asset).** There is no balance table beside the account registry. An `lx_account` record holds one account id, one asset id and one balance, so a DID that touches two assets owns two records - `agent:<did>:main` for the native asset and one `agent:<did>:asset:<hex64>` per other asset - and a per-asset account is kind `LX_ACCOUNT_AGENT_MAIN`, told apart from `:main` by the asset it carries, not by kind. A record's asset is bound when it is opened or by its first credit, and never rebinds; a debit against the wrong asset is refused, not coerced.

Sequences follow the same split. The activity envelope's `account_sequence` is checked against the DID's identity counter, while the transfer set's `actor_sequence` is checked against the sequence account's own counter - by default the debited record, so per `(DID, asset)`. `asset.receive` advances the recipient's counter instead, `asset.mint` the issuance account's, and the protocol fee leg the treasury's. A newly opened per-asset account starts at zero however far its owner's `:main` account has run. Full rules, refusal codes and the account state-root leaf shape are in `spec/layerx-protocol/design.md` §8.2 and §8.3.

See Payments and Fees.

---

## Not modules

| Surface | Where it lives | Module ID |
| --- | --- | --- |
| Identity & authority | Kernel | None - DIDs, keys, grants, rotation, recovery - universal to every activity |
| Oracle / Crossverse | Outside adapter | None - signed oracle activities enter the ordered history; execution never dials out |
| Programs | `src/modules/programs`, `programs/` | `0x09` (`LXP_MODULE_PROGRAMS`) - guest execution and program-owned accounts. Not a ninth `402LXP` writer. [Programs](Programs.md) |

---

## What each module is for

**asset.** Declared ordinals are register (1), pause (2), unpause (3), account_open (4), send (5), receive (6), grant_issue (7), grant_revoke (8), mint (10) and burn (11). Ordinal 9 is reserved for withdraw and is not defined on this module. SEND compiles to a `402LXP` transfer and accepts both `agent:<did>:main` and `agent:<did>:asset:<hex64>` sources owned by the actor. RECEIVE requires a payer grant: one recipient, one account, caps, purpose, expiry. No wildcards. Register `issuer_kind` is `1` native and `2` `paxeer_custody`; that value is not `lx_asset_custody_kind` (`LX_ASSET_CUSTODY_PAXEER = 1`). The authenticated native and hosted submission paths strictly decode and admit register, account_open, send, receive, grant_issue, grant_revoke, mint and burn. Pause/unpause remain excluded from authenticated activity admission, and ordinal 9 returns a reserved-operation refusal.

**escrow.** Lock, capture, release. Terms are module state; money moves only as `402LXP` legs.

**budget.** Fund a ceiling, spend from it, expire or revoke it. Delegation is a grant, not a second wallet.

**stream.** Continuous, metered payment by the unit, drawn under the same conservation rules.

**service.** Offers, commitments, delivery attestations, acceptance, disputes. Payment still walks through `402LXP`.

**perps.** Positions, margin, funding, liquidation, insurance. Losses and fees are transfer legs, not shadow balances.

**governance.** Parameter changes on a timelock. Emergency freezes are named, narrow, and themselves activities.

**bridge.** Deposits and withdrawals against Paxeer custody. The reserve mirror is an ordinary account so conservation still holds.

The opt-in [authenticated custody credit profile](Custody.md) binds real external
deposits to atomic issuance and beneficiary credit while keeping fresh genesis empty.

---

## Programs: module `0x09`

Programs run untrusted guest code on a deterministic WASM runtime under the authority of the activity that invoked them. The kernel registers it as `LXP_MODULE_PROGRAMS = 9` (`include/layerx/lxp_module.h:22`). It never gains balance-writing authority - every monetary effect it produces compiles to a `402LXP` transfer set the kernel applies. The runtime, registry, SDKs, and porting kits live under `programs/`. Full activity, ABI, occupancy, receipt, and event detail is in [Programs](Programs.md).

- **Program-owned accounts.** Derived deterministically from `(program_id, seed)` under a domain-separated hash that is disjoint from principal account ids. No principal can claim or sign for a program account; deriving one conveys no spending authority. A principal can fund a program account but can never authorize debits from it. Program value accounts appear on the money map as `module:programs:value:<account-id>`.
- **Downward-only spending grants.** A program-to-program call can convey a bounded spending grant over the caller's own derived accounts. It narrows only - asset, destination, source, and amount may shrink, never grow. Any widening is refused with the same typed escalation error principal grants use, and no partial transfer set survives.
- **Occupancy settlement.** State that persists is paid for as long as it persists. Occupancy meters namespace bytes held across protocol batches, priced by the fee schedule and charged to the account declared responsible for that namespace, settled as `402LXP` legs and bound into the batch receipt.
- **Protocol-backed balances.** A program's balance is real protocol state, read from the account tree through Merkle proofs - not a registry counter. `402LXP` stays the only writer; programs emit transfer sets and never call `set_balance`.

See [Programs](Programs.md) and `programs/README.md`.

---

## Kernel boundary

The kernel understands identities, accounts, assets, authority, sequences, fees, receipts, checkpoints, and module dispatch. It does not understand funding rates or delivery acceptance.

Each module implements `genesis`, `decode`, `validate` (read-only), `execute` (effects to a buffer only), epoch hooks, and `state_root`. The context handle is the complete capability set: namespaced KV, emit transfer set, emit event, batch timestamp, charge gas. There is no `now()`, no `random()`, no `http()`, and no `set_balance()`.

---

## Start here

- [Home](Home.md)
- [Protocol](Protocol.md): LXC envelope, protocol 3, and the three rules
- [Programs](Programs.md): module `0x09`, CALL vs simulate, guest ABI 2
- [Finality](Finality.md): L0 → L4
- Design § modules
