defmodule Explorer.Repo.Migrations.HotTablesBlockNumberStatistics do
  use Ecto.Migration

  @statistics_target 1000

  @columns [
    {"blocks", "number"},
    {"blocks", "timestamp"},
    {"transactions", "block_number"},
    {"logs", "block_number"},
    {"token_transfers", "block_number"},
    {"internal_transactions", "block_number"},
    {"address_coin_balances", "block_number"}
  ]

  def up do
    Enum.each(@columns, fn {table, column} ->
      execute(~s(ALTER TABLE #{table} ALTER COLUMN "#{column}" SET STATISTICS #{@statistics_target}))
    end)
  end

  def down do
    Enum.each(@columns, fn {table, column} ->
      execute(~s(ALTER TABLE #{table} ALTER COLUMN "#{column}" SET STATISTICS -1))
    end)
  end
end
