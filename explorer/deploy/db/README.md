# Explorer database at 500k+ blocks per day

Operational notes for the Paxeer X Network explorer's PostgreSQL 16 instance: what the new
migrations change, why range partitioning of `logs` and `token_transfers` cannot be delivered
without application changes, and how the read-only API endpoint is wired.

Everything below was checked against a postgres:16 instance carrying the full Blockscout
migration set, not read off the migration files.

## 1. The hot tables as they actually are

| table | primary key | foreign keys | access path on block number |
|---|---|---|---|
| `blocks` | `(hash)` | — | `blocks_number_index`; `one_consensus_block_at_height` unique on `number` where `consensus` |
| `transactions` | `(hash)` | `block_hash → blocks(hash)` cascade | `transactions_block_number_index`, `transactions_recent_collated_index (block_number DESC, index DESC)`, four `(address, block_number, index, inserted_at, hash)` composites |
| `logs` | `(transaction_hash, block_hash, index)` | `block_hash → blocks(hash)`, `transaction_hash → transactions(hash)` cascade | `logs_block_number_ASC__index_ASC_index`, `logs_block_number_DESC__index_DESC_index` |
| `token_transfers` | `(transaction_hash, block_hash, log_index)` | `block_hash → blocks(hash)`, `transaction_hash → transactions(hash)` cascade | `token_transfers_block_number_index` plus ASC/DESC `(block_number, log_index)` pairs |
| `internal_transactions` | `(block_hash, block_index)` | `block_hash → blocks(hash)`, `transaction_hash → transactions(hash)` cascade | **none** — the ordering index was dropped upstream and the three surviving `block_number DESC` indexes are all partial and lead with an address column |
| `address_coin_balances` | `(address_hash, block_number)` | — | `address_coin_balances_block_number_index` |

Nothing references `logs`, `token_transfers` or `internal_transactions` by foreign key.
`block_number` is `NOT NULL` only on `address_coin_balances`; on `transactions`, `logs`,
`token_transfers` and `internal_transactions` it is nullable `integer`.

## 2. Migrations delivered

Three migrations, all idempotent and all with a real `down`. None of them takes a lock
stronger than `SHARE UPDATE EXCLUSIVE`, so they can run against a live indexer.

### `..._hot_tables_storage_parameters`

`ALTER TABLE ... SET (...)` on the six tables above, plus the TOAST relations of
`transactions`, `logs` and `internal_transactions`.

* **`fillfactor`** — at the default of 100 a page has no free space, so every update writes
  the new row version to a different page and therefore has to add an entry to *every* index
  on that row. Below 100 the update can stay on its page and be HOT, and the index trees stop
  bloating. `transactions` gets 85 and `address_coin_balances` 80 because both are updated
  after insertion — transactions get their block assignment and receipt fields, coin balances
  get `value` and `value_fetched_at`. The append-mostly tables get 90.
* **`autovacuum_*_scale_factor = 0.0` with absolute thresholds** — the defaults are
  proportional: 20% dead tuples to vacuum, 10% to analyze. On a table holding a billion rows
  that is 200 million dead tuples, so autovacuum and, worse, autoanalyze never fire; the
  planner's block-number statistics go stale by days and range queries start choosing the
  wrong index. Absolute thresholds trigger on a fixed amount of churn whatever the table size.
* **`autovacuum_vacuum_insert_*`** — append-only tables produce no dead tuples, so the
  dead-tuple trigger alone never marks pages all-visible. Without that, index-only scans fall
  back to heap fetches and the anti-wraparound vacuum arrives as one enormous stall instead of
  a stream of small ones. The insert-based trigger (PostgreSQL 13+) keeps the visibility map
  current.
* **`autovacuum_vacuum_cost_limit`** — the default budget of 200 amounts to a few MB/s of
  vacuum throughput, far below this write rate; a pass over the largest tables would never
  finish.
* **`vacuum_truncate = false`** — truncating trailing empty pages needs a brief
  `ACCESS EXCLUSIVE` lock on the table. On append-only tables there is nothing to reclaim, so
  the only effect is a periodic lock spike against both the indexer and the API.

### `..._internal_transactions_block_number_brin_index`

`internal_transactions` is the one hot table with no access path on `block_number` alone.
Block-range work — reorg cleanup, the internal-transaction delete queue, re-fetching traces
for a range — therefore sequentially scans one of the largest tables in the database.

BRIN is the right shape here rather than a btree. Rows arrive in block order, so the physical
correlation is close to 1 and a summary of a few hundred kilobytes covers a table of hundreds
of gigabytes. More to the point at this write rate, BRIN costs almost nothing on insert,
whereas a btree on `block_number` would add a page write to every trace inserted.
`pages_per_range = 128` trades some scan precision for a smaller index; `autosummarize = on`
summarizes new ranges without waiting for a manual `brin_summarize_new_values`.

The index is built `CONCURRENTLY` outside a migration transaction, so the migration never
blocks writers. If the build is interrupted PostgreSQL leaves an invalid index behind; drop it
by name and re-run.

No BRIN index is added on `logs`, `token_transfers`, `transactions` or `blocks` — each already
has a btree covering the block number, and a second overlapping access path would buy nothing
and cost write amplification on the tables that can least afford it.

### `..._hot_tables_block_number_statistics`

`ALTER COLUMN ... SET STATISTICS 1000` on the six block-number columns and on
`blocks.timestamp`. Block number only ever increases, so with the default 100 histogram
buckets the newest bucket spans days of chain history and the planner badly misjudges the
selectivity of "the last N blocks" — the single most common shape of query the explorer runs.
A larger histogram costs a slower `ANALYZE` and nothing else.

### Rolling back

```
mix ecto.rollback -r Explorer.Repo -n 3
```

`down` resets the storage parameters to the cluster defaults, drops the BRIN index
concurrently, and returns the statistics targets to `-1` (meaning
`default_statistics_target`). Verified on postgres:16 against the full migration set: after
the rollback `pg_class.reloptions` is null on all six tables, the BRIN index is gone and every
touched column is back to `-1`.

### Applying to a live database

Use a short `lock_timeout` so a migration can never queue behind a long-running read:

```
PGOPTIONS='-c lock_timeout=5s' mix ecto.migrate
```

The BRIN migration sets `@disable_ddl_transaction` and `@disable_migration_lock`, so it must
not be run at the same time as another migrator against the same database.

## 3. Range partitioning of `logs` and `token_transfers`: blocked

The brief was to deliver a migration path to `PARTITION BY RANGE (block_number)` **if and only
if** the ORM and the existing unique constraints allow it without touching application code.
They do not. The blockers are structural.

**A. The primary keys do not contain the partition key.** PostgreSQL requires every unique
constraint on a partitioned table to include all partition key columns. Reproduced on
postgres:16 against the real schema:

```
CREATE TABLE logs_partitioned (LIKE logs INCLUDING ALL) PARTITION BY RANGE (block_number);
ERROR:  unique constraint on partitioned table must include all partitioning columns
DETAIL:  PRIMARY KEY constraint on table "logs_partitioned" lacks column "block_number"
         which is part of the partition key.
```

`token_transfers` fails identically. So `block_number` has to join both primary keys.

**B. The bulk importers pin the conflict target in code.**
`Explorer.Chain.Import.Runner.Logs` passes
`conflict_target: [:transaction_hash, :index, :block_hash]` and
`Explorer.Chain.Import.Runner.TokenTransfers` passes
`[:transaction_hash, :log_index, :block_hash]`. PostgreSQL matches `ON CONFLICT (cols)` to a
unique index on exactly those columns, and after (A) no such index can exist. Reproduced
against a partitioned table whose key is widened as (A) demands:

```
INSERT INTO logs_part VALUES (...) ON CONFLICT (transaction_hash, index, block_hash) DO NOTHING;
ERROR:  there is no unique or exclusion constraint matching the ON CONFLICT specification
```

Every import batch would fail. Both runners also carry a second, different conflict target for
the `optimism`/`celo` chain identity, so there are four call sites to change, not two — and
changing them is an application change.

**C. The Ecto schemas declare the composite key.** `Explorer.Chain.Log` and
`Explorer.Chain.TokenTransfer` mark `index`/`log_index`, `block_hash` and `transaction_hash`
as `primary_key: true` under `@primary_key false`. That key drives `Repo.get`, changeset
uniqueness, association loading and the `returning: true` round-trip the importers depend on.
Widening the database key without widening the schema leaves the two disagreeing; widening the
schema changes the public shape of those structs and is, again, application code.

**D. The partition key is nullable today.** `logs.block_number` and
`token_transfers.block_number` both allow NULL. Putting the column in the primary key makes it
`NOT NULL` implicitly, which needs a validated backfill over the existing rows and a guarantee
that no importer path ever writes NULL. Without that, rows are simply rejected:

```
ERROR:  no partition of relation "logs_part" found for row
DETAIL:  Partition key of the failing row contains (block_number) = (null).
```

The alternative — a permanent `DEFAULT` partition — silently collects those rows and defeats
partition pruning for every query that does not constrain `block_number`.

**E. Nothing creates partitions.** `mix ecto.migrate` runs once per deployment; partitions have
to keep appearing ahead of the chain head forever. At 500k blocks per day a 10-million-block
partition is used up in under three weeks. That needs a scheduled maintenance job (pg_partman
or an application-side task), which this deployment does not have and which is code as well.

Conclusion: only item (1) of the brief is delivered. Partitioning remains a deliberate
fork-level change to `apps/explorer/lib/explorer/chain/{log,token_transfer}.ex` and
`apps/explorer/lib/explorer/chain/import/runner/{logs,token_transfers}.ex`, with its own
migration of the existing data. It should not be slipped in under a storage-tuning change.

### The order to use if that work is taken on

Add `block_number NOT NULL` to both tables behind a `NOT VALID` check validated in the
background; change the two schemas and the four runner call sites to a key and conflict target
that include `block_number`; create the partitioned table under a new name with the widened
key; attach the existing table as the first partition after adding a check constraint matching
its bounds, so the attach can skip validation; swap the names in one short transaction; then
run a maintenance job that keeps creating the next partition. Every step before the swap is
reversible; the swap is not. Note that the two foreign keys on each table point *out* at
`blocks` and `transactions`, which a partitioned table may keep, and that nothing points back
at either table — so the foreign keys are not an obstacle, only the keys and the code are.

## 4. Read-only API endpoint

`DATABASE_READ_ONLY_API_URL` is wired end to end and needed no change. The path is:

* `Explorer.Repo.ConfigHelper.get_api_db_url/0` returns it, falling back to `DATABASE_URL`.
* `config/runtime/prod.exs` feeds that into `Explorer.Repo.Replica1` together with
  `POOL_SIZE_API`; `config/runtime/dev.exs` does the same and drops the primary pool default
  from 40 to 30 when the variable is present.
* `Explorer.Repo.Replica1` is declared `read_only: true`, so Ecto generates no write callbacks
  on it, and it is absent from `ConfigHelper.repos/0`, so migrations never target it.
* `Explorer.Chain.select_repo/1` returns `Explorer.Repo.replica/0` for `api?: true`, and the
  `Repo.replica()` call sites in the Etherscan-compatible and API v2 read paths resolve to the
  same module.
* `Explorer.Utility.ReplicaAccessibilityManager` starts only when the variable is set. Every
  ten seconds it reads `pg_is_in_recovery()` and the replay lag; if the lag exceeds
  `REPLICA_MAX_LAG` (`config/runtime.exs`, default five minutes) it sets
  `:replica_inaccessible?`, which makes `Explorer.Repo.replica/0` fall back to the primary
  until the replica catches up.

This was exercised against the local postgres:16 instance with a role holding only `SELECT`:
`get_api_db_url/0` returned the read-only URL, `init_repo_module/2` merged it into the
`Replica1` configuration, `Replica1` connected and served reads against `blocks`, `logs` and
`token_transfers`, an `INSERT` through the same connection was refused with
`insufficient_privilege`, and `Replica1` exports no `insert/2` or `insert_all/3` at all.

Size `POOL_SIZE_API` against the replica's own `max_connections` rather than the primary's —
the two pools are independent.
