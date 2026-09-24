defmodule Explorer.Chain.PaxeerX.UnifiedAccount do
  @moduledoc """
  Reads behind the one-account view of the Paxeer X Network.

  Three things are joined here: the four spellings of an account, one asset list where every
  asset carries a single total beside the parts it is made of, and one activity feed that
  merges the chain's own transactions and token transfers with the LayerX kernel events.

  The `lx_*` tables are written by the LayerX log fetcher and may be absent from a database
  that has not migrated them yet, so every read first checks that the table and the columns it
  needs exist. The columns this module relies on are:

    * `lx_account_bindings` - `evm_address` (bytea), `pax_address`, `did`, `kernel_account`
    * `lx_custody_events` - `event_type`, `asset_id`, `amount`, `evm_address` (bytea),
      `kernel_account`, `block_number`, `log_index`, `transaction_hash` (bytea)
    * `lx_anchors` - `batch_number`
    * `lx_receipts` - `id`

  `lx_anchors` and `lx_receipts` are read whole, so every other column they carry is returned
  as it stands. Amounts are raw units: the native coin in wei, a token in its own smallest
  unit, and a custody asset in whatever unit its `asset_id` is denominated in.
  """

  import Ecto.Query
  import Explorer.Chain, only: [select_repo: 1]

  alias Explorer.Chain
  alias Explorer.Chain.Address
  alias Explorer.Chain.PaxeerX.Identity
  alias Explorer.Chain.{Hash, Token, TokenTransfer, Transaction, Wei}

  @bindings_table "lx_account_bindings"
  @bindings_columns ~w(evm_address pax_address did kernel_account)
  @custody_table "lx_custody_events"
  @custody_columns ~w(event_type asset_id amount evm_address kernel_account block_number log_index transaction_hash)
  @anchors_table "lx_anchors"
  @anchors_columns ~w(batch_number)
  @receipts_table "lx_receipts"
  @receipts_columns ~w(id)

  @custody_signs %{"custody-deposit" => 1, "custody-release" => -1, "emergency-exit" => -1}
  @kernel_signs %{"custody-deposit" => 1, "claim-finalised" => -1, "emergency-exit" => -1}

  @native_asset_id "native"
  @native_decimals 18
  @status_module Explorer.Chain.PaxeerX.Status
  @default_status :instant

  @type identities :: %{
          evm: String.t() | nil,
          pax: String.t() | nil,
          did: String.t() | nil,
          kernel_account: String.t() | nil,
          bound: boolean()
        }

  @type asset :: %{
          asset_id: String.t(),
          denom: String.t() | nil,
          decimals: Decimal.t() | nil,
          token: map() | nil,
          total: Decimal.t(),
          parts: %{chain: Decimal.t(), custody: Decimal.t(), kernel: Decimal.t()}
        }

  @doc """
  The four spellings of the account an EVM address belongs to.

  Only the EVM spelling is known when `lx_account_bindings` is missing or the address has never
  been bound, in which case `bound` is `false`.
  """
  @spec identities(Hash.Address.t(), keyword()) :: identities()
  def identities(%Hash{} = address_hash, options \\ []) do
    repo = select_repo(options)
    evm = Address.checksum(address_hash)

    case binding_row(repo, "evm_address", [address_hash.bytes]) do
      {:ok, row} ->
        %{
          evm: evm,
          pax: row["pax_address"],
          did: row["did"],
          kernel_account: row["kernel_account"],
          bound: true
        }

      :error ->
        %{evm: evm, pax: nil, did: nil, kernel_account: nil, bound: false}
    end
  end

  @doc """
  The EVM address an identity resolves to.

  An EVM identity resolves to itself; every other spelling is looked up in
  `lx_account_bindings` and fails when the table or the binding is absent.
  """
  @spec resolve(Identity.t(), keyword()) :: {:ok, Hash.Address.t()} | :error
  def resolve(%Identity{kind: :evm, evm: evm}, _options), do: {:ok, evm}

  def resolve(%Identity{kind: kind} = identity, options) do
    repo = select_repo(options)
    {column, values} = binding_lookup(kind, identity)

    with {:ok, row} <- binding_row(repo, column, [values]),
         {:ok, address_hash} <- Hash.Address.cast(row["evm_address"]) do
      {:ok, address_hash}
    else
      _ -> :error
    end
  end

  @doc """
  One asset list for an account, each asset carrying a single total and the parts it sums.

  The chain part comes from the coin balance and the current token balances, the custody and
  kernel parts from the signed sums of `lx_custody_events`.
  """
  @spec balances(Hash.Address.t(), String.t() | nil, keyword()) :: [asset()]
  def balances(%Hash{} = address_hash, kernel_account, options \\ []) do
    repo = select_repo(options)

    chain_parts = chain_parts(address_hash, options)
    custody_parts = custody_parts(repo, "evm_address", [address_hash.bytes], @custody_signs)
    kernel_parts = custody_parts(repo, "kernel_account", [account_candidates(kernel_account)], @kernel_signs)

    [chain_parts, custody_parts, kernel_parts]
    |> Enum.flat_map(&Map.keys/1)
    |> Enum.uniq()
    |> Enum.map(&asset(&1, chain_parts, custody_parts, kernel_parts))
    |> Enum.sort_by(& &1.asset_id)
  end

  @doc """
  One activity feed for an account, newest block first.

  Merges the account's transactions, its token transfers and the LayerX custody events it
  appears in. `limit` items are returned at most, so a caller asking for one more than the page
  size can tell whether a next page exists.
  """
  @spec activity(Hash.Address.t(), String.t() | nil, tuple() | nil, pos_integer(), keyword()) :: [map()]
  def activity(%Hash{} = address_hash, kernel_account, paging_key, limit, options \\ []) when is_integer(limit) do
    repo = select_repo(options)

    transactions = transaction_activity(address_hash, paging_key, limit, options)
    token_transfers = token_transfer_activity(address_hash, paging_key, limit, options)
    events = event_activity(repo, address_hash, kernel_account, paging_key, limit)

    (transactions ++ token_transfers ++ events)
    |> Enum.sort_by(&{&1.block_number, &1.ordinal, &1.kind}, :desc)
    |> Enum.take(limit)
    |> Enum.map(&Map.put(&1, :status, ladder_status(&1.block_number, options)))
  end

  @doc """
  A page of anchors, newest batch first, and every column `lx_anchors` carries.
  """
  @spec anchors(integer() | nil, pos_integer(), keyword()) :: [map()]
  def anchors(paging_key, limit, options \\ []) when is_integer(limit) do
    repo = select_repo(options)
    types = column_types(repo, @anchors_table)

    if readable?(types, @anchors_columns) do
      {filter, params} =
        case paging_key do
          nil -> {"", []}
          batch_number -> {"WHERE batch_number < $1", [batch_number]}
        end

      rows(
        repo,
        "SELECT * FROM #{@anchors_table} #{filter} ORDER BY batch_number DESC LIMIT #{limit}",
        params,
        types
      )
    else
      []
    end
  end

  @doc """
  A page of kernel receipts, ordered by id descending as text so that string ids page stably.
  """
  @spec receipts(String.t() | nil, pos_integer(), keyword()) :: [map()]
  def receipts(paging_key, limit, options \\ []) when is_integer(limit) do
    repo = select_repo(options)
    types = column_types(repo, @receipts_table)

    if readable?(types, @receipts_columns) do
      {filter, params} =
        case paging_key do
          nil -> {"", []}
          id -> {"WHERE CAST(id AS text) < $1", [id]}
        end

      rows(
        repo,
        "SELECT * FROM #{@receipts_table} #{filter} ORDER BY CAST(id AS text) DESC LIMIT #{limit}",
        params,
        types
      )
    else
      []
    end
  end

  @doc """
  One kernel receipt by id.
  """
  @spec receipt(String.t(), keyword()) :: {:ok, map()} | :error
  def receipt(id, options \\ []) do
    repo = select_repo(options)
    types = column_types(repo, @receipts_table)

    with true <- readable?(types, @receipts_columns),
         [row] <-
           rows(repo, "SELECT * FROM #{@receipts_table} WHERE CAST(id AS text) = $1 LIMIT 1", [id], types) do
      {:ok, row}
    else
      _ -> :error
    end
  end

  @doc """
  The status ladder rung of a block.

  `Explorer.Chain.PaxeerX.Status` owns the rule. Until that module is compiled into the
  release every block a transaction sits in is reported as `:instant`, which is the rung the
  chain itself guarantees.
  """
  @spec ladder_status(non_neg_integer() | nil, keyword()) :: atom()
  def ladder_status(nil, _options), do: :pending

  def ladder_status(block_number, options) do
    module = @status_module

    if Code.ensure_loaded?(module) and function_exported?(module, :of, 2) do
      apply(module, :of, [block_number, options])
    else
      @default_status
    end
  end

  defp asset(asset_id, chain_parts, custody_parts, kernel_parts) do
    chain = part(chain_parts, asset_id)
    custody = part(custody_parts, asset_id)
    kernel = part(kernel_parts, asset_id)
    metadata = Map.get(chain_parts, asset_id, %{})

    %{
      asset_id: asset_id,
      denom: Map.get(metadata, :denom),
      decimals: Map.get(metadata, :decimals),
      token: Map.get(metadata, :token),
      total: chain |> Decimal.add(custody) |> Decimal.add(kernel),
      parts: %{chain: chain, custody: custody, kernel: kernel}
    }
  end

  defp part(parts, asset_id) do
    parts |> Map.get(asset_id, %{}) |> Map.get(:value, Decimal.new(0))
  end

  defp chain_parts(address_hash, options) do
    coin =
      case Chain.hash_to_address(address_hash, Keyword.put(options, :necessity_by_association, %{})) do
        {:ok, address} -> address.fetched_coin_balance || %Wei{value: Decimal.new(0)}
        _ -> %Wei{value: Decimal.new(0)}
      end

    native = %{
      @native_asset_id => %{
        value: coin.value,
        denom: Explorer.coin(),
        decimals: Decimal.new(@native_decimals),
        token: nil
      }
    }

    address_hash
    |> Chain.fetch_last_token_balances(options)
    |> Enum.reduce(native, fn token_balance, acc ->
      Map.put(acc, Address.checksum(token_balance.token_contract_address_hash), token_part(token_balance))
    end)
  end

  defp token_part(%{token: %Token{} = token} = token_balance) do
    %{
      value: token_balance.value || Decimal.new(0),
      denom: token.symbol,
      decimals: token.decimals,
      token: %{
        address_hash: Address.checksum(token_balance.token_contract_address_hash),
        name: token.name,
        symbol: token.symbol,
        decimals: token.decimals,
        type: token.type
      }
    }
  end

  defp token_part(token_balance) do
    %{value: token_balance.value || Decimal.new(0), denom: nil, decimals: nil, token: nil}
  end

  defp custody_parts(repo, column, params, signs) do
    types = column_types(repo, @custody_table)

    if readable?(types, @custody_columns) do
      repo
      |> rows(
        "SELECT event_type, asset_id, SUM(amount) AS amount FROM #{@custody_table} " <>
          "WHERE #{condition(column)} GROUP BY event_type, asset_id",
        params,
        types
      )
      |> Enum.reduce(%{}, fn row, acc ->
        sign = Map.get(signs, row["event_type"], 0)
        amount = row["amount"] |> to_decimal() |> Decimal.mult(sign)
        asset_id = to_string(row["asset_id"])

        Map.update(acc, asset_id, %{value: amount}, fn %{value: value} ->
          %{value: Decimal.add(value, amount)}
        end)
      end)
    else
      %{}
    end
  end

  defp condition("evm_address"), do: "evm_address = $1"
  defp condition("kernel_account"), do: "kernel_account = ANY($1)"

  defp transaction_activity(address_hash, paging_key, limit, options) do
    Transaction
    |> where(
      [transaction],
      transaction.from_address_hash == ^address_hash or transaction.to_address_hash == ^address_hash
    )
    |> where([transaction], not is_nil(transaction.block_number))
    |> page_transactions(paging_key)
    |> order_by([transaction], desc: transaction.block_number, desc: transaction.index)
    |> limit(^limit)
    |> select([transaction], %{
      block_number: transaction.block_number,
      ordinal: transaction.index,
      timestamp: transaction.block_timestamp,
      transaction_hash: transaction.hash,
      from: transaction.from_address_hash,
      to: transaction.to_address_hash,
      value: transaction.value
    })
    |> select_repo(options).all()
    |> Enum.map(fn item ->
      item
      |> Map.merge(%{kind: "transaction", asset: @native_asset_id})
      |> Map.update!(:value, fn %Wei{value: value} -> value end)
      |> stringify_hashes()
    end)
  end

  defp page_transactions(query, nil), do: query

  defp page_transactions(query, {block_number, index}) do
    where(
      query,
      [transaction],
      transaction.block_number < ^block_number or
        (transaction.block_number == ^block_number and transaction.index < ^index)
    )
  end

  defp token_transfer_activity(address_hash, paging_key, limit, options) do
    TokenTransfer
    |> where(
      [token_transfer],
      token_transfer.from_address_hash == ^address_hash or token_transfer.to_address_hash == ^address_hash
    )
    |> join(:inner, [token_transfer], block in assoc(token_transfer, :block))
    |> where([token_transfer, block], block.consensus == true)
    |> page_token_transfers(paging_key)
    |> order_by([token_transfer], desc: token_transfer.block_number, desc: token_transfer.log_index)
    |> limit(^limit)
    |> select([token_transfer, block], %{
      block_number: token_transfer.block_number,
      ordinal: token_transfer.log_index,
      timestamp: block.timestamp,
      transaction_hash: token_transfer.transaction_hash,
      from: token_transfer.from_address_hash,
      to: token_transfer.to_address_hash,
      value: token_transfer.amount,
      asset: token_transfer.token_contract_address_hash
    })
    |> select_repo(options).all()
    |> Enum.map(fn item ->
      item
      |> Map.put(:kind, "token-transfer")
      |> Map.update!(:asset, &Address.checksum/1)
      |> stringify_hashes()
    end)
  end

  defp page_token_transfers(query, nil), do: query

  defp page_token_transfers(query, {block_number, log_index}) do
    where(
      query,
      [token_transfer],
      token_transfer.block_number < ^block_number or
        (token_transfer.block_number == ^block_number and token_transfer.log_index < ^log_index)
    )
  end

  defp event_activity(repo, address_hash, kernel_account, paging_key, limit) do
    types = column_types(repo, @custody_table)

    if readable?(types, @custody_columns) do
      {filter, params} = event_paging(paging_key, [address_hash.bytes, account_candidates(kernel_account)])

      repo
      |> rows(
        "SELECT event_type, asset_id, amount, evm_address, kernel_account, block_number, log_index, " <>
          "transaction_hash FROM #{@custody_table} " <>
          "WHERE (evm_address = $1 OR kernel_account = ANY($2)) #{filter} " <>
          "ORDER BY block_number DESC, log_index DESC LIMIT #{limit}",
        params,
        types
      )
      |> Enum.map(fn row ->
        %{
          kind: row["event_type"],
          block_number: row["block_number"],
          ordinal: row["log_index"],
          timestamp: nil,
          transaction_hash: row["transaction_hash"],
          from: row["evm_address"],
          to: row["kernel_account"],
          value: to_decimal(row["amount"]),
          asset: to_string(row["asset_id"])
        }
      end)
    else
      []
    end
  end

  defp event_paging(nil, params), do: {"", params}

  defp event_paging({block_number, log_index}, params) do
    {"AND (block_number, log_index) < ($3, $4)", params ++ [block_number, log_index]}
  end

  defp account_candidates(nil), do: []

  defp account_candidates(kernel_account) do
    case Identity.key(kernel_account) do
      nil -> [kernel_account]
      key -> Enum.uniq([kernel_account, key, Identity.kernel_account(key), Identity.did(key)])
    end
  end

  defp binding_lookup(:pax, %Identity{pax: pax}), do: {"pax_address", [pax]}

  defp binding_lookup(:did, %Identity{did: did}), do: {"did", account_candidates(did)}

  defp binding_lookup(:kernel_account, %Identity{kernel_account: kernel_account}),
    do: {"kernel_account", account_candidates(kernel_account)}

  defp binding_row(repo, column, params) do
    types = column_types(repo, @bindings_table)

    if readable?(types, @bindings_columns) do
      condition = if column == "evm_address", do: "evm_address = $1", else: "#{column} = ANY($1)"

      case rows(
             repo,
             "SELECT evm_address, pax_address, did, kernel_account FROM #{@bindings_table} " <>
               "WHERE #{condition} LIMIT 1",
             params,
             types
           ) do
        [row] -> {:ok, row}
        _ -> :error
      end
    else
      :error
    end
  end

  defp rows(repo, statement, params, types) do
    case Ecto.Adapters.SQL.query(repo, statement, params) do
      {:ok, %{columns: columns, rows: rows}} ->
        Enum.map(rows, fn row ->
          columns
          |> Enum.zip(row)
          |> Map.new(fn {column, value} -> {column, decode(value, Map.get(types, column))} end)
        end)

      {:error, _error} ->
        []
    end
  end

  defp decode(nil, _type), do: nil

  defp decode(value, "bytea") when is_binary(value), do: "0x" <> Base.encode16(value, case: :lower)

  defp decode(%Decimal{} = value, _type), do: Decimal.to_string(value, :normal)

  defp decode(value, _type), do: value

  defp to_decimal(%Decimal{} = value), do: value
  defp to_decimal(value) when is_integer(value), do: Decimal.new(value)
  defp to_decimal(value) when is_float(value), do: Decimal.from_float(value)
  defp to_decimal(value) when is_binary(value), do: Decimal.new(value)
  defp to_decimal(nil), do: Decimal.new(0)

  defp stringify_hashes(item) do
    item
    |> Map.update!(:transaction_hash, &to_string/1)
    |> Map.update!(:from, &Address.checksum/1)
    |> Map.update!(:to, &maybe_checksum/1)
  end

  defp maybe_checksum(nil), do: nil
  defp maybe_checksum(value), do: Address.checksum(value)

  defp column_types(repo, table) do
    statement =
      "SELECT column_name::text, data_type::text FROM information_schema.columns " <>
        "WHERE table_schema = current_schema() AND table_name = $1"

    case Ecto.Adapters.SQL.query(repo, statement, [table]) do
      {:ok, %{rows: rows}} -> Map.new(rows, fn [column, type] -> {column, type} end)
      {:error, _error} -> %{}
    end
  end

  defp readable?(types, required), do: Enum.all?(required, &Map.has_key?(types, &1))
end
