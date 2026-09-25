defmodule Explorer.Migrator.HeavyDbIndexOperation.UpdateInternalTransactionsPrimaryKeyTest do
  use Explorer.DataCase, async: false

  alias Explorer.Chain.Cache.BackgroundMigrations
  alias Explorer.Migrator.{HeavyDbIndexOperation, MigrationStatus}
  alias Explorer.Migrator.HeavyDbIndexOperation.UpdateInternalTransactionsPrimaryKey
  alias Explorer.Utility.InternalTransactionHelper

  @migration_name "heavy_indexes_create_internal_transactions_pkey"
  @updated_primary_key ["block_number", "transaction_index", "index"]

  describe "Updates the `internal_transactions` primary key" do
    setup do
      configuration = Application.get_env(:explorer, HeavyDbIndexOperation)
      Application.put_env(:explorer, HeavyDbIndexOperation, check_interval: 200)

      on_exit(fn ->
        Application.put_env(:explorer, HeavyDbIndexOperation, configuration)
      end)

      :ok
    end

    test "records its completion in the migration status table" do
      assert UpdateInternalTransactionsPrimaryKey.migration_name() == @migration_name
      assert UpdateInternalTransactionsPrimaryKey.db_index_operation_status() == :completed
      assert primary_key_columns() == @updated_primary_key

      assert MigrationStatus.get_status(@migration_name) == "completed"
      assert UpdateInternalTransactionsPrimaryKey.migration_finished?()
    end

    test "runs as a no-op against a database it has already changed" do
      %MigrationStatus{status: "completed", updated_at: updated_at} = MigrationStatus.fetch(@migration_name)

      Process.flag(:trap_exit, true)

      {:ok, pid} = UpdateInternalTransactionsPrimaryKey.start_link([])

      assert_receive {:EXIT, ^pid, :normal}, 5_000

      assert primary_key_columns() == @updated_primary_key

      assert %MigrationStatus{status: "completed", updated_at: ^updated_at} = MigrationStatus.fetch(@migration_name)

      assert BackgroundMigrations.get_heavy_indexes_update_internal_transactions_primary_key_finished() == true
      assert InternalTransactionHelper.primary_key_updated?()
    end
  end

  defp primary_key_columns do
    %Postgrex.Result{rows: rows} =
      Repo.query!(
        """
        SELECT key_column_usage.column_name
        FROM information_schema.table_constraints AS table_constraints
        JOIN information_schema.key_column_usage AS key_column_usage
          ON key_column_usage.constraint_name = table_constraints.constraint_name
         AND key_column_usage.constraint_schema = table_constraints.constraint_schema
        WHERE table_constraints.table_schema = 'public'
          AND table_constraints.table_name = 'internal_transactions'
          AND table_constraints.constraint_type = 'PRIMARY KEY'
        ORDER BY key_column_usage.ordinal_position;
        """,
        []
      )

    List.flatten(rows)
  end
end
