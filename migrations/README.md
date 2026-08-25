# LayerX migrations

SQL and genesis artefacts for the C17 LayerX node: import sections, disposable projections, and the history index. This directory is not Ethereum/Solana migration tooling and not Paxeer's in-chain EVM store migrations.

| File | Role |
| --- | --- |
| `0001_genesis_sections.sql` | Genesis import sections, per-asset totals, historical commitments |
| `0001_projection.sql` | Rebuildable projection (balances, receipts, watermark) |
| `0007_history_index.sql` | History index over the append-only log |

The activity log is authoritative. These tables are projections and import bookkeeping; they can be rebuilt.

## Related surfaces

| Kind | Location |
| --- | --- |
| Normative genesis / cutover procedure | [`spec/layerx-protocol/docs/migration.md`](../spec/layerx-protocol/docs/migration.md) |
| Genesis CLI | `cmd/layerx-genesis/` |
| Ethereum / Solana source-chain migration | [`interop/crates/layerx-migrate`](../interop/crates/layerx-migrate/OPERATIONS.md) |
| Paxeer EVM store migrations | `paxeer-network/modules/evm/migrations/` |

`402LXP` is the only balance writer after genesis. Custody reconciliation is against Paxeer, not against a mirror chain.
