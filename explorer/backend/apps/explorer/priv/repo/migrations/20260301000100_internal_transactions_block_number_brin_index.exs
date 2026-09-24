defmodule Explorer.Repo.Migrations.InternalTransactionsBlockNumberBrinIndex do
  use Ecto.Migration

  @disable_ddl_transaction true
  @disable_migration_lock true

  @index_name "internal_transactions_block_number_brin_index"

  def up do
    execute("""
    CREATE INDEX CONCURRENTLY IF NOT EXISTS #{@index_name}
    ON internal_transactions
    USING brin (block_number) WITH (pages_per_range = 128, autosummarize = on)
    """)
  end

  def down do
    execute("DROP INDEX CONCURRENTLY IF EXISTS #{@index_name}")
  end
end
