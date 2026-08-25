<!--
Draft copy for the GitHub wiki page "Protocol".
The wiki has no PR flow, so this file is the reviewable source. After this PR
merges, paste the body below (everything under the first `# Protocol`) into the
wiki page. Do not commit this note to the wiki.
-->

# Protocol

One signed record per action. One doorway for money. One result anyone can replay.

LayerX is the activity, execution, and accounting layer for autonomous agents. Paxeer Network (EVM chain ID `125`) holds custody, checkpoints, bonds, challenges, and exits. A normal payment or agent action does not require a Paxeer transaction.

LayerX and the Paxeer settlement stack now live in one monorepo. Co-location keeps the protocol, the settlement network, the developer surfaces, and their automation auditable in one place while preserving their separate build, release, and trust boundaries — repository co-location grants neither side new authority over the other. LayerX settles on Paxeer; the settlement code lives under `paxeer-network/`.

Normative behavior lives in `spec/` (KVX first). This page is the human read of that design.

---

## Execution vs settlement

| LayerX owns | Paxeer owns |
| --- | --- |
| Agent identities and delegated authority | Asset custody |
| Global activity ordering | Deposits and withdrawals |
| Payments and balances (`402LXP`) | Checkpoint registration |
| Holds, escrow, budgets, streams | Guarantor bonds |
| Service agreements, deliveries, attestations | Checkpoint attestations |
| Trading, positions, funding, liquidation | Slashing for conflicting attestations |
| Programmable guest execution (programs) | Emergency exits and dispute settlement |
| Deterministic state execution, receipts, replay | Final settlement to external assets |
| Data availability and reconstruction | |

Thousands of LayerX activities can collapse into one periodic checkpoint. Custody never leaves Paxeer.

---

## The activity (LXC/1)

Every state-changing operation is one signed activity: a payment, an escrow capture, a budget spend, a stream draw, a service delivery, a perp fill, a governance change, a bridge claim, or a program call.

The wire format is LXC/1 — a canonical binary envelope, not a convenience JSON. Integers are fixed-width and big-endian. There are no optional fields, no maps, and no floating point. Decoding is total: trailing bytes are an error. Re-encoding a valid activity must yield the same bytes.

Typical envelope fields (illustrative names; see the design for encodings):

| Field | Role |
| --- | --- |
| `protocol_version` | Must be enabled for the batch epoch |
| `network_id` | Exact match; blocks cross-network replay |
| `activity_type` | High 16 bits = module id, low 16 = type ordinal |
| `actor_did` | Who is acting |
| `authority` | Primary key, session, or scoped grant |
| `account_sequence` | Must equal `next_sequence[actor]` exactly — gaps are rejected |
| `timestamp_bound` | Window checked against the batch timestamp, never node wall-clock |
| `idempotency_key` | A repeat returns the original receipt with zero new economic effect |
| `fee_limit` | Must cover the deterministically computed fee |
| `payload` / `payload_hash` | Module-specific body; hash checked before parse |
| `signature` | Ed25519 over the canonical prefix |

The protocol verifies the actor and its authority, consumes the sequence, orders the activity globally, applies a deterministic state transition, and returns a signed receipt tied to the resulting state root.

Failed activities still consume sequence, still pay the fee, and still occupy a global sequence number. Effects from the module roll back; bookkeeping does not.

---

## Three design rules

1. **One canonical history.** Every accepted or failed activity receives a global sequence. State roots chain per activity, not only per batch. The append-only activity log is authoritative. Indexes are disposable projections and can be rebuilt by replay.
2. **One financial doorway.** `402LXP` is the only component allowed to write balances. Modules — and programs — emit validated transfer sets; they do not mutate funds themselves. See Payments and Fees.
3. **One reproducible result.** Consensus-critical execution excludes floating point, local clocks, database iteration order, pointer-derived hashes, and other sources of nondeterminism. Replicas and bonded guarantors independently replay batches before a checkpoint is attested to Paxeer.

---

## Identity and authority (kernel)

Identity is kernel, not a module. Agent DIDs are native accounts. Authorization is part of the state machine: primary keys, session keys, scoped capability grants, rotation, recovery, revocation, and expiry.

A grant can be narrowed but not silently widened. Bumping `revocation_sequence` invalidates grants that still reference the old value. Retirement requires a zero balance sheet so value cannot be stranded.

Oracle prices enter as signed activities through a Crossverse adapter — outside the kernel, inside the ordered history. There is no HTTP call from inside a state transition.

---

## Programs: a first-class execution surface

Programs are where untrusted guest code runs. They are a first-class surface alongside the eight economic modules — not a ninth economic module ID. A program executes inside the authority of the activity that invoked it, on a deterministic WASM runtime, and it never gains balance-writing authority: every monetary effect it produces compiles to a `402LXP` transfer set applied by the kernel. The runtime, registry, SDKs, and porting kits live under `programs/`.

Four rules define the programs money story:

- **Program-owned accounts.** A program can own accounts derived deterministically from `(program_id, seed)` under a domain-separated hash. Derivation is a pure function of public inputs, and the domain is disjoint from principal account ids — so no principal can claim or sign for a program account. Deriving an account conveys no spending authority; a principal can fund one but can never authorize debits from it.
- **Downward-only spending grants.** A program-to-program call can convey a bounded spending grant over the caller's own derived accounts. It narrows only: asset, destination, source, and amount can shrink, never grow. Any attempted widening fails with the same typed escalation refusal principal grants use, and no partial transfer set survives.
- **Occupancy settlement.** Storage that persists is paid for as long as it persists. Occupancy meters namespace bytes held across protocol batches, priced by the fee schedule and charged to the account declared responsible for that namespace — settled as ordinary `402LXP` legs and bound into the batch receipt as replay-checkable evidence.
- **Protocol-backed balances.** A program's balance is real protocol state read from the account tree through Merkle proofs, not a bookkeeping column. `402LXP` remains the sole balance writer; programs emit transfer sets and never call `set_balance`.

See Modules and `programs/README.md`.

---

## From activity to receipt

Per activity the kernel, in fixed order:

1. Decode the envelope.
2. Check `network_id`, `protocol_version`, module enablement.
3. Resolve `actor_did` and `authority`; verify the signature.
4. Check `account_sequence == next_sequence[actor]`.
5. Check `timestamp_bound` against the batch timestamp.
6. Check idempotency (hit → original receipt, no new effect).
7. Compute the fee against `fee_limit`.
8. Open a state journal; `validate` then `execute`.
9. Commit on success, roll back on failure.
10. Charge the fee as a `402LXP` transfer, consume the sequence, record the idempotency key, emit the receipt — all four on both success and failure.
11. Recompute the state root and chain it into the receipt.

Receipts are evidence produced by the protocol. No receipt field is supplied by a client.

You pay for the work an activity does — bytes, signatures, state — not a zero-fee line. The specified base fee is 5,000 µUSDX per activity (about half a cent). See Payments and Fees.

---

## Status

Limited beta opens September 7. Source is open for inspection while the public lane is qualified. There is no public RPC, faucet, or explorer for LayerX itself yet, and no public LayerX mainnet — custody and settlement live on Paxeer.

---

## Start here

- Home
- Modules — the eight economic modules and the programs surface
- Finality — L0 → L4
- Design § protocol
