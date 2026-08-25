# LayerX Migration

Normative procedure for migrating from the existing external Go LayerX
(read-only reference) to the C17 implementation in this repository: the
genesis manifest, the import categories, the custody
reconciliation rule, the freeze of the old system, shadow replay, the cutover
runbook, the rollback plan, and the explicit lists of what is and is not carried
over. MUST, MUST NOT, SHOULD and MAY are normative; companion documents under
`spec/layerx-protocol/docs/` are `economics.md` (reserve accounting),
`data-availability.md`, `checkpointing.md` and `state-machine.md`.

This document is the LayerX genesis / cutover procedure (prior Go system → C17).
It is not Ethereum/Solana source-chain migration and not Paxeer EVM store
migration. Those live elsewhere in this monorepo:

| Surface | Location |
|---|---|
| Genesis SQL and projections | [`migrations/`](../../../migrations/README.md) |
| Ethereum / Solana source verifiers | [`interop/crates/layerx-migrate`](../../../interop/crates/layerx-migrate/OPERATIONS.md) |
| Paxeer EVM store migrations | `paxeer-network/modules/evm/migrations/` |
| Paxeer Network (settlement L1) | [`paxeer-network/`](../../../paxeer-network/README.md) |

## 1. Posture

The new protocol starts from an **explicit genesis manifest**. It does not
inherit the old database, does not read PostgreSQL at runtime, and does not treat
an old row as authoritative merely because it exists. Three rules govern
everything below:

1. **Nothing is imported implicitly.** Every unit of value and every state object
   appears in a named, typed, individually rooted section, or it does not exist.
2. **Nothing unbacked is imported.** The manifest total must reconcile exactly
   against Paxeer custody; a shortfall or surplus rejects the entire manifest.
   There is no partial import and no rounding allowance.
3. **The old system is read-only from freeze until retirement**, still serving
   reads for shadow comparison and dispute research, accepting no writes ever.

## 2. Source inventory

The Go implementation's PostgreSQL schema is the extraction source, not the
target model. The tables that carry migratable state:

| Source | Carries |
|---|---|
| `accounts` | `did`, `evm_address`, `balance_usdx`, `escrow_usdx` |
| `holds` | payer, payee, captor, `amount_usdx`, `ref`, expiry, status |
| `withdrawals`, `deposits` | queued payouts not yet settled; confirmed custody inflows keyed by `deposit_tx` |
| `perp_pools` | `liquidity` and `insurance` capital |
| `perp_positions`, `perp_margin_reservations` | open and liquidating positions with `margin_usdx`; `held` order margin |
| `perp_orders`, `perp_funding_entries` | resting and partially filled orders; settled funding and any residue |
| `batches`, `perp_batches` | sealed Merkle roots and anchor transactions |
| `transfers`, `perp_fills`, `perp_liquidations` | history, exported as evidence only |

Amounts are micro-USDX (1e6) in both systems, so no rescaling occurs. Any source
row whose amount is not an exact integer at that scale is a hard extraction
error.

## 3. Genesis manifest format

The manifest is canonical binary, encoded by the same codec as activities
(`docs/wire-format.md`). A JSON rendering exists **only** for human review and is
never an input to genesis.

```c
struct lx_genesis_section {
    uint16_t section_id;        /* see section 5 */
    uint16_t reserved;
    uint32_t entry_count;
    int64_t  total_micro;       /* 0 for value-free sections */
    uint8_t  section_root[32];  /* Merkle root over canonical entries */
};
struct lx_genesis_manifest {
    uint8_t  magic[6];               /* "LXGEN\0" */
    uint16_t format_version;
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t paxeer_chain_id;
    uint8_t  custody_contract[20];
    uint64_t custody_block_number;
    uint8_t  custody_block_hash[32];
    uint8_t  custody_asset[32];      /* asset identifier, USDX */
    int64_t  custody_balance_micro;
    uint8_t  source_freeze_cert[32]; /* hash of the freeze certificate, section 7 */
    uint8_t  source_final_root[32];  /* last sealed root of the Go system */
    uint64_t genesis_timestamp_ms;
    uint16_t section_count;
    /* struct lx_genesis_section sections[section_count]; */
    uint8_t  genesis_state_root[32];
    uint8_t  genesis_root[32];
    /* signature set: sequencer, each guarantor, custody attestor */
};
```

Commitments:

```
entry_leaf   = H("LXP:GEN:ENTRY:v1"    || u16be(section_id) || canonical_encode(entry))
section_root = Merkle(entry_leaf...)                 /* odd nodes promoted, as in DA */
genesis_root = H("LXP:GEN:MANIFEST:v1" || canonical_encode(manifest_without_sigs))
```

`genesis_root` is registered on Paxeer before the first batch is sealed and is
the `previous_state_root` of batch 1, so the whole chain anchors to a manifest
every party signed and can re-derive.

**Genesis obeys 402LXP.** Applying the manifest is not a privileged bulk write:
it is one `MINT_MIRROR(custody_balance_micro)` crediting `system:paxeer-reserve`,
then one atomic transfer set whose legs distribute that value into every
destination account. If any leg fails, genesis fails whole, and invariant R of
`economics.md` section 10 holds from the first commit.

## 4. Worked example

JSON review rendering, abbreviated to one representative entry per section. All
amounts are micro-USDX.

```json
{
  "magic": "LXGEN", "format_version": 1, "protocol_version": 1,
  "network_id": 7791, "paxeer_chain_id": 41337,
  "custody_contract": "0x9f2c...c41a",
  "custody_block_number": 18422991,
  "custody_block_hash": "0x77a1...9e02",
  "custody_asset": "USDX", "custody_balance_micro": 12600000000000,
  "source_freeze_cert": "0x0c4d...ba71",
  "source_final_root": "0x51e8...3d90",
  "genesis_timestamp_ms": 1786752000000,
  "sections": [
    { "id": 1,  "name": "accounts",        "entry_count": 14208, "total_micro": 8100000000000,
      "section_root": "0xa310...44f1",
      "sample": { "did": "did:pax:agent:7Kq...", "sequence": 0,
                  "account": "agent:did:pax:agent:7Kq...:main", "amount_micro": 12500000 } },
    { "id": 2,  "name": "vault_reserves",  "entry_count": 1,     "total_micro": 100000000000,
      "section_root": "0xbb07...9c2e", "note": "custody held against unissued float",
      "sample": { "account": "system:paxeer-reserve", "amount_micro": 100000000000 } },
    { "id": 3,  "name": "open_holds",      "entry_count": 1204,  "total_micro": 250000000000,
      "section_root": "0xc94a...1d55",
      "sample": { "hold_id": "0x3f1a...", "account": "agent:did:pax:agent:7Kq...:escrow:0x3f1a",
                  "payee": "did:pax:agent:2Rt...", "captor": "did:pax:agent:2Rt...",
                  "amount_micro": 4000000, "expires_at_ms": 1786838400000, "ref": "0x9c...2b" } },
    { "id": 4,  "name": "queued_withdrawals", "entry_count": 86, "total_micro": 130000000000,
      "section_root": "0xd0f2...77ab",
      "sample": { "withdrawal_id": "0x81be...", "account": "system:paxeer-withdrawals",
                  "beneficiary_evm": "0x4d5e...81c0", "amount_micro": 250000000 } },
    { "id": 5,  "name": "pools",           "entry_count": 2,     "total_micro": 3000000000000,
      "section_root": "0xe771...0a3c",
      "entries": [ { "account": "system:liquidity:BTC-PERP", "amount_micro": 2000000000000 },
                   { "account": "system:insurance",          "amount_micro": 1000000000000 } ] },
    { "id": 6,  "name": "perps_positions", "entry_count": 312,   "total_micro": 900000000000,
      "section_root": "0xf208...5b6d",
      "sample": { "position_id": "0x77c2...", "owner": "did:pax:agent:7Kq...", "market": "BTC-PERP",
                  "side": "LONG", "contracts": 40, "entry_price_cents": 9812300,
                  "unsettled_funding_micro": 0, "amount_micro": 42000000,
                  "account": "agent:did:pax:agent:7Kq...:margin:0x77c2" } },
    { "id": 7,  "name": "pending_orders",  "entry_count": 77,    "total_micro": 120000000000,
      "section_root": "0x1a44...c803",
      "sample": { "order_id": "0x55de...", "owner": "did:pax:agent:2Rt...", "market": "BTC-PERP",
                  "side": "BUY", "contracts": 10, "filled_contracts": 0, "type": "LIMIT",
                  "limit_price_cents": 9750000, "time_in_force": "GTC",
                  "account": "agent:did:pax:agent:2Rt...:escrow:order:0x55de",
                  "amount_micro": 9800000 } },
    { "id": 8,  "name": "funding_state",   "entry_count": 3,     "total_micro": 0,
      "section_root": "0x2c90...e6f7",
      "sample": { "market": "BTC-PERP", "last_interval_end_ms": 1786751999000,
                  "external_ppb": 41200, "skew_ppb": -8100, "applied_ppb": 33100 } },
    { "id": 9,  "name": "did_evm_bindings","entry_count": 9877,  "total_micro": 0,
      "section_root": "0x3e18...aa20",
      "sample": { "did": "did:pax:agent:7Kq...", "evm_address": "0x4d5e...81c0" } },
    { "id": 10, "name": "receipts_and_roots","entry_count": 4411,"total_micro": 0,
      "section_root": "0x4b6c...12de",
      "sample": { "batch_root": "0x51e8...3d90", "anchor_tx": "0xaf20...9911",
                  "seq_lo": 8210001, "seq_hi": 8214412, "status": "anchored" } }
  ],
  "reconciliation": {
    "section_value_total_micro": 12600000000000,
    "custody_balance_micro":     12600000000000,
    "difference_micro": 0,
    "genesis_transfer_legs_total_micro": 12500000000000
  },
  "genesis_state_root": "0x6d21...ff08",
  "genesis_root": "0x8ea4...30b7"
}
```

The arithmetic `layerx-genesis --verify` recomputes:

```
  8_100_000_000_000  accounts            250_000_000_000  open holds
    100_000_000_000  vault reserves      130_000_000_000  queued withdrawals
  3_000_000_000_000  pools               900_000_000_000  position margin
                                         120_000_000_000  pending order margin
 12_600_000_000_000  == custody_balance_micro       (difference must be 0)
 12_500_000_000_000  == genesis transfer legs (total minus mirror float)
```

## 5. Import categories

Each category is a separate section with its own root, count and total; sections
8, 9 and 10 carry no value and MUST report `total_micro = 0`.

| ID | Category | Source | Target account or object | Rejects if |
|---|---|---|---|---|
| 1 | USDX balances | `accounts.balance_usdx` | `agent:<did>:main` | negative; DID malformed; no key material recoverable; duplicate DID |
| 2 | Vault reserves | custody attestation minus issued value | `system:paxeer-reserve` | negative; not equal to `custody_balance - Σ(sections 1,3..7)` |
| 3 | Open holds | `holds` where `status='open'` | `agent:<payer>:escrow:<hold_id>` | expired at freeze; payer balance already consumed; captor unknown; amount ≤ 0 |
| 4 | Queued withdrawals | `withdrawals` where `status='queued'` | `system:paxeer-withdrawals` | already settled on Paxeer; beneficiary binding missing; amount ≤ 0 |
| 5 | Liquidity and insurance pools | `perp_pools` | `system:liquidity:<market>`, `system:insurance` | negative; pool id not in the locked registry |
| 6 | Open perps positions | `perp_positions` where `status IN ('OPEN','LIQUIDATING')` | `agent:<did>:margin:<position_id>` plus a position object | `contracts <= 0`; `entry_price_cents <= 0`; `unsettled_funding_usdx != 0`; margin below maintenance at the freeze mark; more than one open position per `(owner, market)` |
| 7 | Pending orders | `perp_orders` in `ACCEPTED`, `RESTING` or `PARTIALLY_FILLED`, with their `held` reservations | `agent:<did>:escrow:order:<order_id>` plus an order object | reservation missing or mismatched; `filled_contracts > contracts`; market not active |
| 8 | Funding state | `perp_funding_entries`, market funding cursors | per-market funding object | any position has non-zero unsettled funding; interval cursor ahead of the freeze timestamp |
| 9 | DID-to-EVM bindings | `accounts.evm_address` | identity binding object | address malformed; two DIDs claiming one address without an explicit governance override |
| 10 | Outstanding receipts and anchored roots | `batches`, `perp_batches` | historical anchor commitments | a root claims an anchor tx that is absent from Paxeer; sequence ranges overlap or leave a gap |

Notes for the extractor: `accounts.escrow_usdx` is a **net-reserve audit counter,
not spendable value** — a cross-check during extraction, then discarded, never
imported as a balance. Holds in the old system lock funds inside the payer's own
account row, so import *moves* that value into a real escrow subaccount rather
than reproducing a column. Funding must be fully settled before freeze (section 7
step 4), which is why section 8 carries no value: importing unsettled funding
would require inventing an interval boundary, which is not deterministic.

## 6. Custody reconciliation rule

`Σ_{s ∈ value sections} total_micro == custody_balance_micro`, evaluated with
exact `int64` arithmetic and **tolerance zero**.

- A **shortfall** (sections exceed custody) means the old system recorded value
  custody does not back. The manifest is rejected: no trimming, no pro-rata
  haircut, no "import what is backed" mode exists.
- A **surplus** (custody exceeds sections) means custody holds value no account
  claims, typically unclaimed or unprocessed deposits. Default behaviour is
  rejection; the only permitted resolutions are to place it explicitly in section
  2 as mirror float (the worked example's 100 000 USDX) or, under a governance
  flag recorded in the manifest, to credit it to `system:insurance` as a named
  entry.
- `custody_balance_micro` MUST be read at one finalised Paxeer block
  (`custody_block_number`, `custody_block_hash`) **after** the freeze certificate,
  so no in-flight deposit moves underneath the reading.
- Every guarantor re-verifies the reconciliation independently before signing,
  and the first checkpoint's reserve audit verifies it again.

## 7. Read-only freeze of the Go system

Executed in order; each step is verified before the next begins.

1. **Announce.** Publish the freeze window at least `LX_FREEZE_NOTICE`
   (recommended 72 hours) in advance.
2. **Stop new exposure.** Every perps market to `REDUCE_ONLY`, then `PAUSED`;
   reject new orders, holds and streams at the API edge.
3. **Drain in-flight work.** Let open captures, releases and settlements finish;
   expire due holds through the normal path, never by hand.
4. **Settle funding** to a clean interval boundary, so every position has
   `unsettled_funding_usdx = 0`.
5. **Seal and anchor** the final `batches` and `perp_batches` rows and wait for
   anchor confirmation on Paxeer.
6. **Revoke writes.** Drop `INSERT`, `UPDATE`, `DELETE` from every application
   role, leaving `SELECT`; deploy the API with all write endpoints returning
   HTTP 503 and a fixed body `{"code":"LX_FROZEN"}`.
7. **Snapshot** consistently at a recorded LSN — this snapshot, not the live
   database, is the extraction source.
8. **Certify.** Produce the freeze certificate (snapshot LSN, per-table row
   counts and value totals, final roots, anchor tx hashes, timestamp) signed by
   the operator and every guarantor key; its hash becomes `source_freeze_cert`.

After step 6 the old system is a read replica of history. It MUST NOT be
unfrozen except through the rollback procedure in section 10.

## 8. Shadow-replay comparison

Shadow replay runs from feature-completeness of the C implementation to the
go/no-go gate. It proves the new implementation reproduces the old system's
*accepted outcomes*, not its internal representations.

**Method.** Export the old system's accepted operation history in commit order
from the snapshot, translate each operation into the equivalent activity envelope
with `tools/shadow-translate`, feed the stream into a C node started from a
genesis manifest built at the history's start point, and compare after every
operation.

| Class | Compared | Tolerance |
|---|---|---|
| Acceptance | accepted vs rejected, and the rejection reason class | exact |
| Balances | every touched account's post-balance | exact, 0 µUSDX |
| Conservation | Σ debits == Σ credits per set; bucket totals | exact |
| Perps fills | contracts, price, notional, fee, realised PnL | exact |
| Liquidations | trigger point, waterfall leg amounts, insurance draw | exact |
| Funding | per-position transfer amount per interval | exact |
| Receipts | semantic fields (`from`, `to`, `amount`, before/after balances) | exact; encodings differ by design and are not compared |
| Ordering | relative order of operations affecting a common account | exact |

**Known-divergence allowlist.** A divergence is acceptable only with a written
justification, a test pinning the new behaviour, and sign-off recorded in the
manifest review. Anticipated entries: rounding corrections where the Go code
rounded in the user's favour against the rounding law of `economics.md` section
1; timestamp determinism, where batch-supplied time replaces `now()`; and
idempotency semantics, where one economic result per key is enforced more
strictly than `perp_idempotency` did.

**Acceptance criteria.** At least 30 consecutive days and 10^6 replayed
operations covering every activity type and at least one each of liquidation,
funding interval, deposit, withdrawal, hold capture and hold expiry; zero
unexplained divergences; every allowlisted divergence covered by a test; and
byte-identical state roots between two independently built C nodes on different
architectures across the whole stream.

## 9. Cutover runbook

| When | Step | Verification gate | Abort if |
|---|---|---|---|
| T-14d | Freeze notice published; canary agents onboarded | acknowledged by top-50 agents by volume | fewer than 80% acknowledge |
| T-7d | Guarantors bonded on Paxeer; DA storage provisioned | `N >= 4` bonded, each passing a DA possession drill | any guarantor unbonded or failing |
| T-72h | Dry-run genesis from a rehearsal snapshot; full node boot | `layerx-genesis --verify` returns 0; difference 0 | any nonzero difference |
| T-24h | Shadow replay final report | section 8 acceptance criteria met | any unexplained divergence |
| T-0 | Execute freeze (section 7) | freeze certificate signed by all parties | any step fails verification |
| T+1h | Read custody at a finalised block; build the real manifest | difference 0; all sections rooted | reconciliation fails |
| T+2h | Guarantors independently rebuild and sign the manifest, then `genesis_root` is registered on Paxeer | threshold reached on an identical root; registration finalised | any guarantor derives a different root; the transaction reverts or is reorged |
| T+4h | Boot the C sequencer; apply genesis; seal batch 1 (empty) | batch 1 attested by threshold; reserve audit passes | any attestation failure |
| T+5h | Open deposits and transfers only | 100 real transfers reconcile | any reserve mismatch |
| T+24h | Open escrow, budget, stream and service modules | receipts verified by external agents | any invariant violation |
| T+72h | Perps to `REDUCE_ONLY`, then `ACTIVE` after one clean funding interval | capacity gate satisfied; funding conserves exactly | capacity below required, or funding residue misrouted |
| T+30d | Retire the old system to cold archive | no open dispute references it | any open dispute |

## 10. Rollback plan

Three windows, with sharply different costs.

**Window A — before `genesis_root` registration (to T+3h).** Fully reversible:
discard the manifest, restore write permissions on the Go system, revert the API
deployment, resume. Cost is the freeze window; no on-chain footprint.

**Window B — after registration, before the first value-moving activity.**
Reversible on-chain. Governance calls `abandonGenesis(genesis_root)` on the
custody contract within `LX_GENESIS_ABANDON_WINDOW` (recommended 6 hours),
invalidating the root and re-permitting a later registration. The C system is
wiped, the Go system unfrozen as in window A. No agent has been credited or
debited under the new protocol, so no reverse manifest is needed.

**Window C — after the first finalised checkpoint containing user activity.**
The point of no return for a simple rollback. Reversal becomes a *forward
migration in reverse*: freeze the C system by the section 7 procedure, export a
reverse manifest from the last finalised state root, reconcile it against custody
with tolerance zero, and import it into the Go system through a purpose-built
importer. It is expensive, needs the same guarantor signatures and shadow
verification, and MUST NOT be treated as a routine escape hatch.

In every window agent funds stay reachable: before registration through the Go
system, after it through the C system or, if that has halted, through the Paxeer
emergency exit (`docs/data-availability.md`, section 9).

## 11. What is not carried over

Discarded outright, per the architectural brief and the locked decisions:

- **PostgreSQL structures as the protocol definition.** Table shapes, UUID keys,
  `TIMESTAMPTZ` columns and `JSONB` response blobs are extraction artefacts; the
  protocol is the canonical binary codec plus the state machine, and indexes are
  rebuildable SQLite projections.
- **HTTP endpoints as the canonical wire protocol.** JSON/HTTP survives only as
  an optional gateway that never defines consensus behaviour.
- **In-memory authentication challenges as authority state.** Session keys,
  capability grants, delegation limits, revocation and expiry become explicit
  state inside the state machine, not HTTP middleware.
- **Direct Crossverse access inside execution.** Prices enter only as signed
  oracle activities already in history; no external call occurs during a
  transition.
- **Development settlement fallbacks.** No mock or bypass settlement path exists
  in the runtime, in any build configuration.
- **Process-local SSE as the authoritative event mechanism.** Events are
  Merkle-committed in `event_merkle_root` and served from the availability set; a
  stream is a convenience view.
- **Implicit background timing as consensus behaviour.** Expiries, funding
  intervals and timeouts advance from batch-supplied deterministic timestamps,
  never a goroutine's wall clock.
- **Docs that mix historical plans with current behaviour**, floating-point
  anywhere in value computation, and `status` strings duplicating what the ledger
  already derives.

## 12. What is preserved

| Concept | How it survives |
|---|---|
| DID-native accounts | the DID remains the account identity, now with explicit subaccounts |
| Escrow-bounded spending | holds become real escrow subaccounts moved by 402LXP transfers |
| Fully reserved assets | invariant R of `economics.md` section 10, checked every checkpoint |
| Signed receipts | `402LXPReceipt` with before/after balances as evidence, plus inclusion proofs after checkpointing |
| Merkle commitments | six roots per batch header plus the DA manifest root |
| Idempotent execution | one economic result per idempotency key, enforced in state |
| Crash recovery | append-only log plus journalled writes, recovery tested at every write boundary |
| Deterministic perps arithmetic | the integer formulas of `economics.md` section 9, retained verbatim |
| Fail-closed market data | stale or divergent oracle input pauses markets rather than guessing |
| Staged rollout and emergency modes | `OFF`, `SHADOW`, `CANARY`, `ACTIVE`, `REDUCE_ONLY`, `PAUSED` retained as governance state |
| Paxeer custody and escape guarantees | custody, checkpoints, disputes and emergency exit, with no ordinary activity on Paxeer |

## 13. Qualification gates before cutover

Cutover is blocked until all hold: byte-identical replay across machines and
architectures; deterministic state roots from at least 10^6 activities; crash
recovery verified at every write boundary; network-partition and sequencer-loss
tests passed; malformed-activity and signature fuzzing clean; overflow and
rounding proofs discharged; guarantor disagreement and equivocation producing the
specified slashes; data-unavailability simulation halting finalisation and
permitting full exit; emergency exit executed on a Paxeer testnet for every
account class; reserve reconciliation at difference zero; shadow comparison
meeting section 8; a canary with real agents; and independent C API and
wire-protocol conformance suites passing.

## 14. Implementation map

| Path | Responsibility |
|---|---|
| `cmd/layerx-genesis/` | build, `--verify`, and apply the manifest; reconciliation report |
| `migrations/extract/` | read-only extractors against the frozen snapshot, one per section |
| `src/protocol/genesis.c` | manifest decode, section roots, `MINT_MIRROR` plus the genesis transfer set |
| `tools/shadow-translate/`, `tools/shadow-compare/` | operation-to-envelope translation, per-operation comparison, divergence classification and allowlist |
| `contracts/` | `registerGenesis`, `abandonGenesis`, custody reading helpers |
| `tests/migration/` | worked-example vectors, a rejection case for every rule in section 5, rollback drills |
