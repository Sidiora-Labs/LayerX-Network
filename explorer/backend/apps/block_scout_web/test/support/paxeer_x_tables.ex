defmodule BlockScoutWeb.PaxeerXTables do
  @moduledoc """
  Creates the `lx_*` tables the LayerX log fetcher writes and inserts rows into them.

  The migrations that own these tables are not part of every database, which is exactly the
  case the API guards against, so the tests create them inside their own sandbox transaction
  and leave them out when they want to exercise the guard.
  """

  alias Explorer.Repo

  @doc """
  Creates every `lx_*` table the API reads.
  """
  @spec create_all() :: :ok
  def create_all do
    create_account_bindings()
    create_custody_events()
    create_anchors()
    create_receipts()
  end

  @spec create_account_bindings() :: :ok
  def create_account_bindings do
    execute("""
    CREATE TABLE lx_account_bindings (
      evm_address bytea NOT NULL,
      pax_address text,
      did text,
      kernel_account text,
      block_number bigint,
      log_index integer
    )
    """)
  end

  @spec create_custody_events() :: :ok
  def create_custody_events do
    execute("""
    CREATE TABLE lx_custody_events (
      event_type text NOT NULL,
      asset_id bytea NOT NULL,
      amount numeric NOT NULL,
      evm_address bytea,
      kernel_account text,
      block_number bigint NOT NULL,
      log_index integer NOT NULL,
      transaction_hash bytea NOT NULL
    )
    """)
  end

  @spec create_anchors() :: :ok
  def create_anchors do
    execute("""
    CREATE TABLE lx_anchors (
      batch_number bigint NOT NULL,
      checkpoint_id bytea NOT NULL,
      state_root bytea,
      receipt_root bytea,
      status text,
      block_number bigint,
      log_index integer
    )
    """)
  end

  @spec create_receipts() :: :ok
  def create_receipts do
    execute("""
    CREATE TABLE lx_receipts (
      id text NOT NULL,
      batch_number bigint,
      account text,
      kind text,
      block_number bigint
    )
    """)
  end

  @doc """
  Inserts one row into `lx_account_bindings`.
  """
  @spec insert_binding(binary(), String.t(), String.t(), String.t()) :: :ok
  def insert_binding(evm_address_bytes, pax_address, did, kernel_account) do
    execute(
      "INSERT INTO lx_account_bindings (evm_address, pax_address, did, kernel_account) VALUES ($1, $2, $3, $4)",
      [evm_address_bytes, pax_address, did, kernel_account]
    )
  end

  @doc """
  Inserts one row into `lx_custody_events`.
  """
  @spec insert_custody_event(keyword()) :: :ok
  def insert_custody_event(fields) do
    execute(
      "INSERT INTO lx_custody_events (event_type, asset_id, amount, evm_address, kernel_account, block_number, " <>
        "log_index, transaction_hash) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
      [
        Keyword.fetch!(fields, :event_type),
        Keyword.fetch!(fields, :asset_id),
        Keyword.fetch!(fields, :amount),
        Keyword.get(fields, :evm_address),
        Keyword.get(fields, :kernel_account),
        Keyword.fetch!(fields, :block_number),
        Keyword.fetch!(fields, :log_index),
        Keyword.fetch!(fields, :transaction_hash)
      ]
    )
  end

  @doc """
  Inserts one row into `lx_anchors`.
  """
  @spec insert_anchor(non_neg_integer(), binary()) :: :ok
  def insert_anchor(batch_number, checkpoint_id) do
    execute(
      "INSERT INTO lx_anchors (batch_number, checkpoint_id, status, block_number, log_index) " <>
        "VALUES ($1, $2, $3, $4, $5)",
      [batch_number, checkpoint_id, "submitted", batch_number, 0]
    )
  end

  @doc """
  Inserts one row into `lx_receipts`.
  """
  @spec insert_receipt(String.t(), non_neg_integer()) :: :ok
  def insert_receipt(id, batch_number) do
    execute("INSERT INTO lx_receipts (id, batch_number, kind, block_number) VALUES ($1, $2, $3, $4)", [
      id,
      batch_number,
      "activity",
      batch_number
    ])
  end

  defp execute(statement, params \\ []) do
    Ecto.Adapters.SQL.query!(Repo, statement, params)

    :ok
  end
end
