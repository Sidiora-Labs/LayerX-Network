defmodule Explorer.Repo.Migrations.HotTablesStorageParameters do
  use Ecto.Migration

  @heap_parameters [
    {"blocks",
     [
       {"fillfactor", "90"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "20000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "100000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "10000"},
       {"autovacuum_vacuum_cost_limit", "4000"},
       {"vacuum_truncate", "false"}
     ]},
    {"transactions",
     [
       {"fillfactor", "85"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "100000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "500000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "50000"},
       {"autovacuum_vacuum_cost_limit", "4000"},
       {"vacuum_truncate", "false"}
     ]},
    {"logs",
     [
       {"fillfactor", "90"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "200000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "1000000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "100000"},
       {"autovacuum_vacuum_cost_limit", "6000"},
       {"vacuum_truncate", "false"}
     ]},
    {"token_transfers",
     [
       {"fillfactor", "90"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "200000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "1000000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "100000"},
       {"autovacuum_vacuum_cost_limit", "6000"},
       {"vacuum_truncate", "false"}
     ]},
    {"internal_transactions",
     [
       {"fillfactor", "90"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "200000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "1000000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "100000"},
       {"autovacuum_vacuum_cost_limit", "6000"},
       {"vacuum_truncate", "false"}
     ]},
    {"address_coin_balances",
     [
       {"fillfactor", "80"},
       {"autovacuum_vacuum_scale_factor", "0.0"},
       {"autovacuum_vacuum_threshold", "100000"},
       {"autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"autovacuum_vacuum_insert_threshold", "500000"},
       {"autovacuum_analyze_scale_factor", "0.0"},
       {"autovacuum_analyze_threshold", "50000"},
       {"autovacuum_vacuum_cost_limit", "4000"},
       {"vacuum_truncate", "false"}
     ]}
  ]

  @toast_parameters [
    {"transactions",
     [
       {"toast.autovacuum_vacuum_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_threshold", "100000"},
       {"toast.autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_insert_threshold", "500000"},
       {"toast.autovacuum_vacuum_cost_limit", "4000"}
     ]},
    {"logs",
     [
       {"toast.autovacuum_vacuum_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_threshold", "200000"},
       {"toast.autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_insert_threshold", "1000000"},
       {"toast.autovacuum_vacuum_cost_limit", "6000"}
     ]},
    {"internal_transactions",
     [
       {"toast.autovacuum_vacuum_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_threshold", "200000"},
       {"toast.autovacuum_vacuum_insert_scale_factor", "0.0"},
       {"toast.autovacuum_vacuum_insert_threshold", "1000000"},
       {"toast.autovacuum_vacuum_cost_limit", "6000"}
     ]}
  ]

  def up do
    Enum.each(@heap_parameters ++ @toast_parameters, fn {table, parameters} ->
      execute("ALTER TABLE #{table} SET (#{set_clause(parameters)})")
    end)
  end

  def down do
    Enum.each(@heap_parameters ++ @toast_parameters, fn {table, parameters} ->
      execute("ALTER TABLE #{table} RESET (#{reset_clause(parameters)})")
    end)
  end

  defp set_clause(parameters) do
    Enum.map_join(parameters, ", ", fn {name, value} -> "#{name} = #{value}" end)
  end

  defp reset_clause(parameters) do
    Enum.map_join(parameters, ", ", fn {name, _value} -> name end)
  end
end
