<!--
Draft copy for the GitHub wiki page "Modules".
The wiki has no PR flow, so this file is the reviewable source. After this PR
merges, paste the body below (everything under the first `# Modules`) into the
wiki page. Do not commit this note to the wiki.
-->

# Modules

Eight economic modules on a kernel that owns identity and authority.

Programs and oracle intake are separate surfaces — not extra module IDs.

---

## The registered set (0x01–0x08)

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

Module IDs are stable and never reused. They occupy the high 16 bits of `activity_type`. An unknown or epoch-disabled module is refused — not best-effort decoded.

Runtime sources live under `src/modules/` (`asset`, `escrow`, `budget`, `stream`, `service`, `perps`, `governance`, `bridge`, plus `programs` as a separate surface). Since the monorepo integration, the Paxeer settlement stack these modules checkpoint to lives in the same repository under `paxeer-network/` (EVM chain ID `125`).

---

## One doorway for money

`402LXP` is the sole balance writer. That is the feature, not a caveat.

Modules never call `set_balance`. They emit transfer sets — one or more legs with a single authorization context, a single sequence, and a single receipt. All legs commit, or none do. Per asset, Σ debits = Σ credits.

Locked funds are real accounts, not hidden columns:

```
agent:<did>:main
agent:<did>:budget:<id>
agent:<did>:escrow:<id>
agent:<did>:stream:<id>
agent:<did>:margin:<position>
module:programs:value:<account-id>
system:fees
system:paxeer-reserve
```

Opening a position is a transfer into a margin account. Capturing escrow is a transfer out of an escrow account. Ordinary modules do not mint and do not burn.

See Payments and Fees.

---

## Not modules

| Surface | Where it lives | Why it is not `0x09` / `0x0A` |
| --- | --- | --- |
| Identity & authority | Kernel | DIDs, keys, grants, rotation, recovery — universal to every activity |
| Oracle / Crossverse | Outside adapter | Signed oracle activities enter the ordered history; execution never dials out |
| Programs | Separate runtime (`src/modules/programs`, `programs/`) | Guest execution and program-owned accounts — a first-class surface, not a ninth economic module ID on this table |

---

## What each module is for

**asset.** SEND and RECEIVE compile to the same internal transfer. RECEIVE requires a payer grant: one recipient, one account, caps, purpose, expiry. No wildcards.

**escrow.** Lock, capture, release. Terms are module state; money moves only as `402LXP` legs.

**budget.** Fund a ceiling, spend from it, expire or revoke it. Delegation is a grant, not a second wallet.

**stream.** Continuous, metered payment by the unit, drawn under the same conservation rules.

**service.** Offers, commitments, delivery attestations, acceptance, disputes. Payment still walks through `402LXP`.

**perps.** Positions, margin, funding, liquidation, insurance. Losses and fees are transfer legs, not shadow balances.

**governance.** Parameter changes on a timelock. Emergency freezes are named, narrow, and themselves activities.

**bridge.** Deposits and withdrawals against Paxeer custody. The reserve mirror is an ordinary account so conservation still holds.

---

## Programs: the first-class execution surface

Programs run untrusted guest code on a deterministic WASM runtime under the authority of the activity that invoked them. A program is a first-class surface, not a ninth economic module, and it never gains balance-writing authority — every monetary effect it produces compiles to a `402LXP` transfer set the kernel applies. The runtime, registry, SDKs, and porting kits live under `programs/`.

- **Program-owned accounts.** Derived deterministically from `(program_id, seed)` under a domain-separated hash that is disjoint from principal account ids. No principal can claim or sign for a program account; deriving one conveys no spending authority. A principal can fund a program account but can never authorize debits from it. Program value accounts appear on the money map as `module:programs:value:<account-id>`.
- **Downward-only spending grants.** A program-to-program call can convey a bounded spending grant over the caller's own derived accounts. It narrows only — asset, destination, source, and amount may shrink, never grow. Any widening is refused with the same typed escalation error principal grants use, and no partial transfer set survives.
- **Occupancy settlement.** State that persists is paid for as long as it persists. Occupancy meters namespace bytes held across protocol batches, priced by the fee schedule and charged to the account declared responsible for that namespace, settled as `402LXP` legs and bound into the batch receipt.
- **Protocol-backed balances.** A program's balance is real protocol state, read from the account tree through Merkle proofs — not a registry counter. `402LXP` stays the only writer; programs emit transfer sets and never call `set_balance`.

See `programs/README.md`.

---

## Kernel boundary

The kernel understands identities, accounts, assets, authority, sequences, fees, receipts, checkpoints, and module dispatch. It does not understand funding rates or delivery acceptance.

Each module implements `genesis`, `decode`, `validate` (read-only), `execute` (effects to a buffer only), epoch hooks, and `state_root`. The context handle is the complete capability set: namespaced KV, emit transfer set, emit event, batch timestamp, charge gas. There is no `now()`, no `random()`, no `http()`, and no `set_balance()`.

---

## Start here

- Home
- Protocol — LXC envelope and the three rules
- Finality — L0 → L4
- Design § modules
