defmodule Explorer.Chain.PaxeerX.UnifiedAccount do
  @moduledoc """
  Reads behind the one-account view of the Paxeer X Network.

  Three things are joined here: the four spellings of an account, one asset list where every
  asset carries a single total beside the parts it is made of, and one activity feed that
  merges the chain's own transactions and token transfers with the LayerX kernel events.

  Every read goes through the schemas that own the `lx_*` tables —
  `Explorer.Chain.PaxeerX.{AccountBinding, CustodyEvent, Anchor, Receipt}` — and through their
  `only_consensus_query/0`, so a reorged block's rows drop out of the account view the same way
  token transfers do.

  Amounts are raw units: the native coin in wei, a token in its own smallest unit, and a
  custody asset in whatever unit its `asset_id` is denominated in. The rung every item carries
  is decided by `Explorer.Chain.PaxeerX.Status` from one read of
  `Explorer.Chain.PaxeerX.Finality.heights/1`, so a whole page of items costs one anchor read.
  """

  import Ecto.Query
  import Explorer.Chain, only: [select_repo: 1]

  alias Explorer.Chain
  alias Explorer.Chain.Address
  alias Explorer.Chain.PaxeerX.{AccountBinding, Anchor, CustodyEvent, Finality, Identity, Receipt, Status}
  alias Explorer.Chain.PaxeerX.Finality.Heights
  alias Explorer.Chain.{Hash, Token, TokenTransfer, Transaction, Wei}

  @native_asset_id "native"
  @native_decimals 18

  @direction_signs %{deposit: 1, withdrawal: -1, balance_delta: 0}

  @type identities :: %{
          evm: String.t() | nil,
          pax: String.t() | nil,
          did: String.t() | nil,
          kernel_account: String.t() | nil,
          bound: boolean()
        }

  @type asset_ref :: %{
          id: String.t(),
          denom: String.t(),
          symbol: String.t() | nil,
          decimals: integer() | nil
        }

  @type asset :: %{
          asset: asset_ref(),
          total: Decimal.t(),
          parts: %{chain: Decimal.t(), custody: Decimal.t(), kernel: Decimal.t()}
        }

  @type activity_item :: %{
          kind: String.t(),
          hash: String.t(),
          block_number: non_neg_integer(),
          ordinal: non_neg_integer(),
          timestamp: DateTime.t() | nil,
          asset: asset_ref() | nil,
          amount: Decimal.t() | nil,
          counterparty: String.t() | nil,
          side: :chain | :kernel,
          status: Status.rung()
        }

  @doc """
  The four spellings of the account an EVM address belongs to.

  The latest binding log of the address decides: only the EVM spelling is known while no
  consensus block has bound it, or while the latest log unbound it, in which case `bound` is
  `false`.
  """
  @spec identities(Hash.Address.t(), keyword()) :: identities()
  def identities(%Hash{} = address_hash, options \\ []) do
    evm = Address.checksum(address_hash)

    query = latest_binding_query(dynamic([binding], binding.evm_address_hash == ^address_hash))

    case select_repo(options).one(query) do
      %AccountBinding{bound: true} = binding ->
        %{
          evm: evm,
          pax: binding.pax_address,
          did: binding.layerx_did,
          kernel_account: binding.layerx_account,
          bound: true
        }

      _unbound ->
        %{evm: evm, pax: nil, did: nil, kernel_account: nil, bound: false}
    end
  end

  @doc """
  The EVM address an identity resolves to.

  An EVM identity resolves to itself; every other spelling is looked up in
  `lx_account_bindings`, where the newest log of that identity decides: an identity no
  consensus block has bound, or whose newest log unbound it, resolves to nothing.
  """
  @spec resolve(Identity.t(), keyword()) :: {:ok, Hash.Address.t()} | :error
  def resolve(%Identity{kind: :evm, evm: evm}, _options), do: {:ok, evm}

  def resolve(%Identity{} = identity, options) do
    case identity |> binding_condition() |> latest_binding_query() |> select_repo(options).one() do
      %AccountBinding{bound: true, evm_address_hash: %Hash{} = address_hash} -> {:ok, address_hash}
      _unbound -> :error
    end
  end

  @doc """
  One asset list for an account, each asset carrying a single total and the parts it sums.

  The chain part comes from the coin balance and the current token balances, the custody and
  kernel parts from the signed sums of `lx_custody_events`: the custody part from the events
  the EVM address acts in, the kernel part from the events its kernel account is credited or
  debited in.
  """
  @spec balances(Hash.Address.t(), String.t() | nil, keyword()) :: [asset()]
  def balances(%Hash{} = address_hash, kernel_account, options \\ []) do
    repo = select_repo(options)

    chain_parts = chain_parts(address_hash, options)
    custody_parts = custody_parts(repo, dynamic([event], event.address_hash == ^address_hash))

    kernel_parts =
      case kernel_account_hash(kernel_account) do
        nil -> %{}
        account -> custody_parts(repo, dynamic([event], event.account == ^account))
      end

    [chain_parts, custody_parts, kernel_parts]
    |> Enum.flat_map(&Map.keys/1)
    |> Enum.uniq()
    |> Enum.map(&asset(&1, chain_parts, custody_parts, kernel_parts))
    |> Enum.sort_by(& &1.asset.id)
  end

  @doc """
  One activity feed for an account, newest block first.

  Merges the account's transactions, its token transfers and the LayerX custody events it
  appears in. `limit` items are returned at most, so a caller asking for one more than the page
  size can tell whether a next page exists.
  """
  @spec activity(Hash.Address.t(), String.t() | nil, {integer(), integer()} | nil, pos_integer(), keyword()) :: [
          activity_item()
        ]
  def activity(%Hash{} = address_hash, kernel_account, paging_key, limit, options \\ []) when is_integer(limit) do
    repo = select_repo(options)
    heights = heights(options)

    transactions = transaction_activity(address_hash, paging_key, limit, options)
    token_transfers = token_transfer_activity(address_hash, paging_key, limit, options)
    events = event_activity(repo, address_hash, kernel_account, paging_key, limit)

    (transactions ++ token_transfers ++ events)
    |> Enum.sort_by(&{&1.block_number, &1.ordinal, &1.kind}, :desc)
    |> Enum.take(limit)
    |> Enum.map(&Map.put(&1, :status, Status.of(&1.block_number, heights).rung))
  end

  @doc """
  A page of anchor checkpoints, newest batch first, one row per batch.

  A checkpoint is logged again every time it climbs a rung of the anchor ladder, so the newest
  row of each batch is the one the page carries.
  """
  @spec anchors(integer() | nil, pos_integer(), keyword()) :: [map()]
  def anchors(paging_key, limit, options \\ []) when is_integer(limit) do
    Anchor.only_consensus_query()
    |> page_anchors(paging_key)
    |> distinct([anchor], desc: anchor.batch_number)
    |> order_by([anchor], desc: anchor.batch_number, desc: anchor.block_number, desc: anchor.log_index)
    |> limit(^limit)
    |> select([anchor, block: block], %{
      batch_number: anchor.batch_number,
      checkpoint_id: anchor.checkpoint_id,
      checkpoint_height: anchor.kernel_height,
      sealed_height: anchor.sealed_height,
      state_root: anchor.state_root,
      block_number: anchor.block_number,
      timestamp: block.timestamp
    })
    |> select_repo(options).all()
  end

  @doc """
  A page of kernel receipts, newest receipt id first, one row per receipt.

  A receipt is logged again on every rung of its verification lattice, so the newest row of
  each receipt id is the one the page carries. Each receipt also carries the settlement rung
  of the block it was last seen in.
  """
  @spec receipts(String.t() | nil, pos_integer(), keyword()) :: [map()]
  def receipts(paging_key, limit, options \\ []) when is_integer(limit) do
    heights = heights(options)

    Receipt.only_consensus_query()
    |> page_receipts(paging_key)
    |> distinct([receipt], desc: receipt.receipt_id)
    |> order_by([receipt], desc: receipt.receipt_id, desc: receipt.block_number, desc: receipt.log_index)
    |> limit(^limit)
    |> select_receipt()
    |> select_repo(options).all()
    |> Enum.map(&put_receipt_rung(&1, heights))
  end

  @doc """
  One kernel receipt by id, as its newest row reports it.
  """
  @spec receipt(String.t(), keyword()) :: {:ok, map()} | :error
  def receipt(id, options \\ []) do
    with {:ok, receipt_id} <- Hash.Full.cast(id),
         %{} = row <- select_repo(options).one(receipt_query(receipt_id)) do
      {:ok, put_receipt_rung(row, heights(options))}
    else
      _missing -> :error
    end
  end

  @doc """
  The settlement ladder rung of a block, with every height and source that decided it.

  `Explorer.Chain.PaxeerX.Status` owns the rule and `Explorer.Chain.PaxeerX.Finality` owns the
  heights it decides from.
  """
  @spec ladder_status(non_neg_integer() | nil, keyword()) :: Status.t()
  def ladder_status(block_number, options \\ []), do: Status.of(block_number, heights(options))

  @doc """
  The settlement heights one page of items is decided against.
  """
  @spec heights(keyword()) :: Heights.t()
  def heights(options \\ []), do: Finality.heights(options)

  defp receipt_query(receipt_id) do
    Receipt.only_consensus_query()
    |> where([receipt], receipt.receipt_id == ^receipt_id)
    |> order_by([receipt], desc: receipt.block_number, desc: receipt.log_index)
    |> limit(1)
    |> select_receipt()
  end

  defp latest_binding_query(condition) do
    from(binding in AccountBinding.only_consensus_query(),
      where: ^condition,
      order_by: [desc: binding.block_number, desc: binding.log_index],
      limit: 1
    )
  end

  defp binding_condition(%Identity{kind: :pax, pax: pax}), do: dynamic([binding], binding.pax_address == ^pax)

  defp binding_condition(%Identity{kind: :did, did: did}), do: kernel_binding_condition(did)

  defp binding_condition(%Identity{kind: :kernel_account, kernel_account: kernel_account}),
    do: kernel_binding_condition(kernel_account)

  defp kernel_binding_condition(account) do
    candidates = account_candidates(account)

    dynamic([binding], binding.layerx_did in ^candidates or binding.layerx_account in ^candidates)
  end

  defp asset(asset_id, chain_parts, custody_parts, kernel_parts) do
    chain = part(chain_parts, asset_id)
    custody = part(custody_parts, asset_id)
    kernel = part(kernel_parts, asset_id)

    %{
      asset: asset_ref(asset_id, [chain_parts, custody_parts, kernel_parts]),
      total: chain |> Decimal.add(custody) |> Decimal.add(kernel),
      parts: %{chain: chain, custody: custody, kernel: kernel}
    }
  end

  defp asset_ref(asset_id, sources) do
    Enum.find_value(sources, %{id: asset_id, denom: asset_id, symbol: nil, decimals: nil}, fn parts ->
      parts |> Map.get(asset_id, %{}) |> Map.get(:asset)
    end)
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

    native = %{@native_asset_id => %{value: coin.value, asset: native_asset_ref()}}

    address_hash
    |> Chain.fetch_last_token_balances(options)
    |> Enum.reduce(native, fn token_balance, acc ->
      Map.put(acc, Address.checksum(token_balance.token_contract_address_hash), token_part(token_balance))
    end)
  end

  defp token_part(token_balance) do
    id = Address.checksum(token_balance.token_contract_address_hash)

    %{value: token_balance.value || Decimal.new(0), asset: token_asset_ref(id, token_balance.token)}
  end

  defp token_asset_ref(id, %Token{} = token) do
    %{id: id, denom: token.symbol || id, symbol: token.symbol, decimals: decimals(token.decimals)}
  end

  defp token_asset_ref(id, _token), do: %{id: id, denom: id, symbol: nil, decimals: nil}

  defp custody_parts(repo, condition) do
    query =
      from(event in CustodyEvent.only_consensus_query(),
        where: ^condition,
        where: not is_nil(event.asset_id) and not is_nil(event.amount),
        group_by: [event.asset_id, event.direction],
        select: {event.asset_id, event.direction, sum(event.amount)}
      )

    query
    |> repo.all()
    |> Enum.reduce(%{}, fn {asset_id, direction, amount}, acc ->
      id = to_string(asset_id)
      signed = Decimal.mult(amount, Map.fetch!(@direction_signs, direction))

      Map.update(acc, id, %{value: signed, asset: custody_asset_ref(id)}, fn part ->
        %{part | value: Decimal.add(part.value, signed)}
      end)
    end)
  end

  defp custody_asset_ref(id), do: %{id: id, denom: id, symbol: nil, decimals: nil}

  defp transaction_activity(address_hash, paging_key, limit, options) do
    Transaction
    |> where(
      [transaction],
      transaction.from_address_hash == ^address_hash or transaction.to_address_hash == ^address_hash
    )
    |> where([transaction], not is_nil(transaction.block_number))
    |> page_by_index(paging_key)
    |> order_by([transaction], desc: transaction.block_number, desc: transaction.index)
    |> limit(^limit)
    |> select([transaction], %{
      block_number: transaction.block_number,
      ordinal: transaction.index,
      timestamp: transaction.block_timestamp,
      hash: transaction.hash,
      from: transaction.from_address_hash,
      to: transaction.to_address_hash,
      amount: transaction.value
    })
    |> select_repo(options).all()
    |> Enum.map(fn item ->
      item
      |> Map.merge(%{kind: "transaction", side: :chain, asset: native_asset_ref()})
      |> Map.update!(:amount, fn %Wei{value: value} -> value end)
      |> counterparty(address_hash)
    end)
  end

  defp token_transfer_activity(address_hash, paging_key, limit, options) do
    TokenTransfer
    |> where(
      [token_transfer],
      token_transfer.from_address_hash == ^address_hash or token_transfer.to_address_hash == ^address_hash
    )
    |> join(:inner, [token_transfer], block in assoc(token_transfer, :block), as: :block)
    |> join(:left, [token_transfer], token in assoc(token_transfer, :token), as: :token)
    |> where([block: block], block.consensus == true)
    |> page_by_log_index(paging_key)
    |> order_by([token_transfer], desc: token_transfer.block_number, desc: token_transfer.log_index)
    |> limit(^limit)
    |> select([token_transfer, block: block, token: token], %{
      block_number: token_transfer.block_number,
      ordinal: token_transfer.log_index,
      timestamp: block.timestamp,
      hash: token_transfer.transaction_hash,
      from: token_transfer.from_address_hash,
      to: token_transfer.to_address_hash,
      amount: token_transfer.amount,
      contract_address_hash: token_transfer.token_contract_address_hash,
      symbol: token.symbol,
      decimals: token.decimals
    })
    |> select_repo(options).all()
    |> Enum.map(fn item ->
      id = Address.checksum(item.contract_address_hash)

      item
      |> Map.merge(%{
        kind: "token_transfer",
        side: :chain,
        asset: %{id: id, denom: item.symbol || id, symbol: item.symbol, decimals: decimals(item.decimals)}
      })
      |> Map.drop([:contract_address_hash, :symbol, :decimals])
      |> counterparty(address_hash)
    end)
  end

  defp event_activity(repo, address_hash, kernel_account, paging_key, limit) do
    condition =
      case kernel_account_hash(kernel_account) do
        nil -> dynamic([event], event.address_hash == ^address_hash)
        account -> dynamic([event], event.address_hash == ^address_hash or event.account == ^account)
      end

    query =
      CustodyEvent.only_consensus_query()
      |> where(^condition)
      |> page_by_log_index(paging_key)
      |> order_by([event], desc: event.block_number, desc: event.log_index)
      |> limit(^limit)
      |> select([event, block: block], %{
        kind: event.kind,
        block_number: event.block_number,
        ordinal: event.log_index,
        timestamp: block.timestamp,
        hash: event.transaction_hash,
        address_hash: event.address_hash,
        account: event.account,
        amount: event.amount,
        asset_id: event.asset_id
      })

    query
    |> repo.all()
    |> Enum.map(fn item ->
      %{
        kind: to_string(item.kind),
        side: :kernel,
        block_number: item.block_number,
        ordinal: item.ordinal,
        timestamp: item.timestamp,
        hash: to_string(item.hash),
        amount: item.amount,
        asset: item.asset_id && custody_asset_ref(to_string(item.asset_id)),
        counterparty: event_counterparty(item, address_hash)
      }
    end)
  end

  defp event_counterparty(%{address_hash: address_hash, account: account}, requested) do
    if address_hash == requested do
      account && to_string(account)
    else
      address_hash && Address.checksum(address_hash)
    end
  end

  defp counterparty(item, address_hash) do
    other = if item.from == address_hash, do: item.to, else: item.from

    item
    |> Map.drop([:from, :to])
    |> Map.merge(%{hash: to_string(item.hash), counterparty: other && Address.checksum(other)})
  end

  defp native_asset_ref do
    %{id: @native_asset_id, denom: Explorer.coin(), symbol: Explorer.coin(), decimals: @native_decimals}
  end

  defp decimals(nil), do: nil
  defp decimals(%Decimal{} = value), do: Decimal.to_integer(value)
  defp decimals(value) when is_integer(value), do: value

  defp page_by_index(query, nil), do: query

  defp page_by_index(query, {block_number, index}) do
    where(
      query,
      [item],
      item.block_number < ^block_number or (item.block_number == ^block_number and item.index < ^index)
    )
  end

  defp page_by_log_index(query, nil), do: query

  defp page_by_log_index(query, {block_number, log_index}) do
    where(
      query,
      [item],
      item.block_number < ^block_number or (item.block_number == ^block_number and item.log_index < ^log_index)
    )
  end

  defp page_anchors(query, nil), do: query

  defp page_anchors(query, batch_number) when is_integer(batch_number),
    do: where(query, [anchor], anchor.batch_number < ^batch_number)

  defp page_receipts(query, nil), do: query

  defp page_receipts(query, id) when is_binary(id) do
    case Hash.Full.cast(id) do
      {:ok, receipt_id} -> where(query, [receipt], receipt.receipt_id < ^receipt_id)
      :error -> query
    end
  end

  defp select_receipt(query) do
    select(query, [receipt, block: block], %{
      id: receipt.receipt_id,
      account: receipt.account,
      verification_status: receipt.status,
      payload_hash: receipt.payload_hash,
      transaction_hash: receipt.transaction_hash,
      block_number: receipt.block_number,
      timestamp: block.timestamp
    })
  end

  defp put_receipt_rung(row, heights), do: Map.put(row, :status, Status.of(row.block_number, heights).rung)

  defp account_candidates(nil), do: []

  defp account_candidates(account) do
    case Identity.key(account) do
      nil -> [account]
      key -> Enum.uniq([account, key, Identity.kernel_account(key), Identity.did(key)])
    end
  end

  defp kernel_account_hash(nil), do: nil

  defp kernel_account_hash(kernel_account) do
    with key when is_binary(key) <- Identity.key(kernel_account),
         {:ok, %Hash{} = hash} <- Hash.Full.cast("0x" <> key) do
      hash
    else
      _unparsed -> nil
    end
  end
end
