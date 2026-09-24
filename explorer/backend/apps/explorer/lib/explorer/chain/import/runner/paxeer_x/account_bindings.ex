defmodule Explorer.Chain.Import.Runner.PaxeerX.AccountBindings do
  @moduledoc """
  Bulk imports `t:Explorer.Chain.PaxeerX.AccountBinding.t/0`.

  Rows are keyed by the transaction hash and the log index of the log they were decoded
  from, and a repeated log is left untouched rather than rewritten, so re-reading a
  range of blocks is idempotent.
  """

  require Ecto.Query

  alias Ecto.{Changeset, Multi, Repo}
  alias Explorer.Chain.Import
  alias Explorer.Chain.PaxeerX.AccountBinding
  alias Explorer.Prometheus.Instrumenter

  @behaviour Import.Runner

  # milliseconds
  @timeout 60_000

  @type imported :: [AccountBinding.t()]

  @impl Import.Runner
  def ecto_schema_module, do: AccountBinding

  @impl Import.Runner
  def option_key, do: :paxeer_x_account_bindings

  @impl Import.Runner
  @spec imported_table_row() :: %{:value_description => binary(), :value_type => binary()}
  def imported_table_row do
    %{
      value_type: "[#{ecto_schema_module()}.t()]",
      value_description: "List of `t:#{ecto_schema_module()}.t/0`s"
    }
  end

  @impl Import.Runner
  @spec run(Multi.t(), list(), map()) :: Multi.t()
  def run(multi, changes_list, %{timestamps: timestamps} = options) do
    insert_options =
      options
      |> Map.get(option_key(), %{})
      |> Map.take(~w(on_conflict timeout)a)
      |> Map.put_new(:timeout, @timeout)
      |> Map.put(:timestamps, timestamps)

    Multi.run(multi, :insert_paxeer_x_account_bindings, fn repo, _ ->
      Instrumenter.block_import_stage_runner(
        fn -> insert(repo, changes_list, insert_options) end,
        :block_referencing,
        :paxeer_x_account_bindings,
        :paxeer_x_account_bindings
      )
    end)
  end

  @impl Import.Runner
  def timeout, do: @timeout

  @spec insert(Repo.t(), [map()], %{required(:timeout) => timeout(), required(:timestamps) => Import.timestamps()}) ::
          {:ok, [AccountBinding.t()]}
          | {:error, [Changeset.t()]}
  def insert(repo, changes_list, %{timeout: timeout, timestamps: timestamps} = _options) when is_list(changes_list) do
    # Enforce PaxeerX.AccountBinding ShareLocks order (see docs: sharelock.md)
    ordered_changes_list = Enum.sort_by(changes_list, &{&1.transaction_hash, &1.log_index})

    {:ok, inserted} =
      Import.insert_changes_list(
        repo,
        ordered_changes_list,
        for: AccountBinding,
        returning: true,
        timeout: timeout,
        timestamps: timestamps,
        conflict_target: [:transaction_hash, :log_index],
        on_conflict: :nothing
      )

    {:ok, inserted}
  end
end
