defmodule EthereumJSONRPC.PaxeerX do
  @moduledoc """
  Ethereum JSONRPC variant for the Paxeer X Network EVM RPC.

  The Paxeer X EVM RPC is a go-ethereum derivative served on top of a CometBFT
  consensus engine. It answers the whole `eth_*` surface a Blockscout indexer
  needs, but four of its answers are not Ethereum-shaped. This module is the one
  place that states each divergence and the handling it requires; the variant
  callbacks themselves are the Geth ones, because the tracing, pending
  transaction and beneficiary surfaces are unmodified go-ethereum.

  Selected with `ETHEREUM_JSONRPC_VARIANT=paxeer_x`.

  ## 1. Cosmos-originated transactions carry type `0xffffffff`

  Messages that originate on the Cosmos side of the node (bank sends, wasm
  executions) are surfaced to the EVM through a shell receipt whose transaction
  type is `math.MaxUint32`, i.e. `0xffffffff`, deliberately outside the
  `0x00..0x7f` range EIP-2718 reserves for typed transactions.

  `Explorer.Chain.Transaction.type` is a Postgres `integer` (4 bytes, signed),
  so the wire value 4_294_967_295 cannot be stored: importing it raises rather
  than being silently truncated. `normalize_transaction_type/1` maps the marker
  onto a configurable in-range type - by default `0x7f`, the last EIP-2718 type
  byte, which no transaction envelope uses - so that Cosmos-originated activity
  is imported and stays distinguishable from a legacy (`0x00`) transaction.
  The mapping is applied by `EthereumJSONRPC.Transaction.elixir_to_params/1`
  (the transaction object) and by `EthereumJSONRPC.Receipt.elixir_to_params/1`
  (the shell receipt, which is where the marker actually appears - the block
  body renders these entries with a zero type).

  Configure with:

      config :ethereum_jsonrpc, EthereumJSONRPC.PaxeerX,
        cosmos_transaction_type: 0x7F

  ## 2. `receiptsRoot` is the previous block's `LastResultsHash`

  The header mapping is CometBFT's, not Ethereum's: `stateRoot` is the
  `AppHash`, `transactionsRoot` is the `DataHash`, and `receiptsRoot` is the
  `LastResultsHash`, which commits to the results of the *preceding* block. It
  is therefore neither a Merkle-Patricia root over this block's receipts nor
  unique per block: every empty block in a run shares one value.

  Nothing in the import pipeline may treat `receiptsRoot` as a checksum over the
  receipts it fetched, and nothing may treat two blocks with an equal
  `receiptsRoot` as the same block. `receipts_root_verifiable?/0` states this
  for callers that would otherwise reach for such a check; the value itself is
  imported verbatim, as returned.

  ## 3. `newHeads` headers disagree with the block body

  A `newHeads` notification is rendered from the CometBFT header alone, before
  the EVM-side block body is assembled, so for one and the same height it
  reports an all-zero `logsBloom`, an all-zero `sha3Uncles` (the body reports
  the RLP hash of the empty uncle list), a `gasUsed` summed over every CometBFT
  transaction rather than over the EVM subset, and the raw `DataHash` as
  `transactionsRoot` (the body substitutes the empty-transactions hash when the
  EVM-filtered list is empty). It also omits `size`, `totalDifficulty`,
  `transactions` and `uncles` altogether.

  A notification is therefore a hint that a height exists, never block data.
  `fetch_block_by_new_head/2` fetches the body by the announced hash, checks
  that the body agrees with the announced number, and refetches by number when
  it does not - the body always wins.

  ## 4. `eth_syncing` always errors

  The RPC registers `eth_syncing` and then rejects it with JSON-RPC error
  -32000; the node has no notion of a sync target to report. Consensus-level
  catch-up is not observable over `eth_*` at all. `fetch_syncing_status/1`
  reports an errored `eth_syncing` as synced, because on this chain the error is
  the node's steady state and not a degraded one.

  ## 5. Native coin decimals

  The chain's bank denomination is `uhpx` with 6 decimals. The EVM state layer
  exposes it as an 18-decimal coin by multiplying every bank amount by 10^12, so
  `eth_getBalance` and transaction values are ordinary wei and need no rescaling
  on the way into the explorer. Only a reconciliation against Cosmos-side supply
  has to divide by `bank_denom_scaling_factor/0`.

  `native_coin_decimals/0` is what the UI should render the native coin with. It
  defaults to 18 - the EVM-facing figure, which is what every balance the
  indexer reads is denominated in - and is configurable for a deployment that
  chooses to present the 6-decimal bank figure instead:

      config :ethereum_jsonrpc, EthereumJSONRPC.PaxeerX,
        native_coin_decimals: 18
  """

  import EthereumJSONRPC, only: [json_rpc: 2, quantity_to_integer: 1, request: 1]

  alias EthereumJSONRPC.{Blocks, Geth}

  @behaviour EthereumJSONRPC.Variant

  @cosmos_transaction_type 0xFFFF_FFFF
  @default_cosmos_transaction_type 0x7F

  @bank_denom_decimals 6
  @evm_denom_decimals 18
  @default_native_coin_decimals @evm_denom_decimals

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_beneficiaries(block_range, json_rpc_named_arguments), to: Geth

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_internal_transactions(transactions_params, json_rpc_named_arguments), to: Geth

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_block_internal_transactions(block_numbers, json_rpc_named_arguments), to: Geth

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_first_trace(transactions_params, json_rpc_named_arguments), to: Geth

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_transaction_raw_traces(transaction_params, json_rpc_named_arguments), to: Geth

  @impl EthereumJSONRPC.Variant
  defdelegate fetch_pending_transactions(json_rpc_named_arguments), to: Geth

  @doc """
  The transaction type a Cosmos-originated (shell) receipt carries on the wire:
  `0xffffffff`, i.e. `math.MaxUint32`.
  """
  @spec cosmos_transaction_type() :: pos_integer()
  def cosmos_transaction_type, do: @cosmos_transaction_type

  @doc """
  The in-range transaction type Cosmos-originated transactions are stored with.

  Read from `config :ethereum_jsonrpc, EthereumJSONRPC.PaxeerX,
  cosmos_transaction_type: integer`, defaulting to `0x7f`.
  """
  @spec stored_cosmos_transaction_type() :: non_neg_integer()
  def stored_cosmos_transaction_type do
    case config(:cosmos_transaction_type) do
      type when is_integer(type) and type >= 0 -> type
      _ -> @default_cosmos_transaction_type
    end
  end

  @doc """
  Whether `type`, as a quantity or an already decoded integer, is the
  Cosmos-originated marker.
  """
  @spec cosmos_transaction_type?(EthereumJSONRPC.quantity() | non_neg_integer() | nil) :: boolean()
  def cosmos_transaction_type?(nil), do: false
  def cosmos_transaction_type?(type) when is_integer(type), do: type == @cosmos_transaction_type

  def cosmos_transaction_type?(type) when is_binary(type),
    do: quantity_to_integer(type) == @cosmos_transaction_type

  def cosmos_transaction_type?(_), do: false

  @doc """
  Maps the Cosmos-originated transaction type onto the stored one and leaves
  every other type untouched.
  """
  @spec normalize_transaction_type(EthereumJSONRPC.quantity() | non_neg_integer() | nil) ::
          EthereumJSONRPC.quantity() | non_neg_integer() | nil
  def normalize_transaction_type(type) do
    if cosmos_transaction_type?(type) do
      stored_cosmos_transaction_type()
    else
      type
    end
  end

  @doc """
  Makes a decoded transaction map importable.

  Two rewrites, both no-ops on a transaction an Ethereum client would emit:

    * a `"type"` of `0xffffffff` becomes `stored_cosmos_transaction_type/0`, as
      described in the module documentation;

    * a missing or `nil` `"value"` becomes `0`. The Paxeer X block encoder
      renders a Cosmos-originated wasm execution with a null `value` - the
      message moves no EVM balance and the encoder leaves the field unset -
      and a transaction without a `"value"` matches none of
      `EthereumJSONRPC.Transaction`'s conversion clauses, so it would raise
      `FunctionClauseError` and take the whole block down with it.
  """
  @spec normalize_transaction(map()) :: map()
  def normalize_transaction(elixir) when is_map(elixir) do
    elixir
    |> normalize_decoded_type()
    |> normalize_decoded_value()
  end

  defp normalize_decoded_type(%{"type" => type} = elixir) do
    if cosmos_transaction_type?(type) do
      %{elixir | "type" => stored_cosmos_transaction_type()}
    else
      elixir
    end
  end

  defp normalize_decoded_type(elixir), do: elixir

  defp normalize_decoded_value(elixir) do
    case Map.fetch(elixir, "value") do
      {:ok, value} when not is_nil(value) -> elixir
      _ -> Map.put(elixir, "value", 0)
    end
  end

  @doc """
  Stamps receipt params with the stored Cosmos-originated type when the raw
  receipt carries the marker.

  The block body renders Cosmos-originated entries with a zero type, so the
  shell receipt is the only place the marker appears; receipt params are merged
  over transaction params during import, which is what carries the type onto the
  imported transaction.
  """
  @spec put_cosmos_transaction_type(map(), map()) :: map()
  def put_cosmos_transaction_type(params, %{"type" => type}) when is_map(params) do
    if cosmos_transaction_type?(type) do
      Map.put(params, :type, stored_cosmos_transaction_type())
    else
      params
    end
  end

  def put_cosmos_transaction_type(params, _elixir) when is_map(params), do: params

  @doc """
  `false`: `receiptsRoot` is the previous block's CometBFT `LastResultsHash` and
  commits to nothing in this block, so it can neither be recomputed from the
  fetched receipts nor used to tell two blocks apart.
  """
  @spec receipts_root_verifiable?() :: boolean()
  def receipts_root_verifiable?, do: false

  @doc """
  Whether a `newHeads` notification and a fetched block body describe the same
  block.

  Only the height and the hash are compared. Every other header field is
  expected to differ: the notification is rendered from the CometBFT header
  before the EVM block body exists.
  """
  @spec new_head_matches_block?(map(), map()) :: boolean()
  def new_head_matches_block?(new_head, block_params) when is_map(new_head) and is_map(block_params) do
    with {:ok, number} <- new_head_number(new_head),
         {:ok, hash} <- new_head_hash(new_head) do
      number == block_params[:number] and hashes_equal?(hash, block_params[:hash])
    else
      _ -> false
    end
  end

  @doc """
  Turns a `newHeads` notification into block params, trusting only the fetched
  body.

  The body is fetched by the announced hash. When that fetch yields no block, or
  yields a body whose number disagrees with the announced one, the block is
  refetched by number and that result is returned instead. The notification's
  own header fields are never used.
  """
  @spec fetch_block_by_new_head(map(), EthereumJSONRPC.json_rpc_named_arguments()) ::
          {:ok, Blocks.t()} | {:error, reason :: term()}
  def fetch_block_by_new_head(new_head, json_rpc_named_arguments) when is_map(new_head) do
    with {:ok, number} <- new_head_number(new_head),
         {:ok, hash} <- new_head_hash(new_head) do
      case EthereumJSONRPC.fetch_blocks_by_hash([hash], json_rpc_named_arguments) do
        {:ok, %Blocks{blocks_params: [block_params], errors: []} = blocks} ->
          if new_head_matches_block?(new_head, block_params) do
            {:ok, blocks}
          else
            EthereumJSONRPC.fetch_blocks_by_numbers([number], json_rpc_named_arguments)
          end

        {:ok, %Blocks{}} ->
          EthereumJSONRPC.fetch_blocks_by_numbers([number], json_rpc_named_arguments)

        {:error, _reason} ->
          EthereumJSONRPC.fetch_blocks_by_numbers([number], json_rpc_named_arguments)
      end
    end
  end

  @doc """
  Reads `eth_syncing`, reporting an errored call as synced.

  The Paxeer X RPC answers `eth_syncing` with JSON-RPC error -32000 at all
  times; that is the node's normal answer, not a fault, so it is reported as
  `{:ok, :synced}`. A node that does answer is read normally, so this stays
  correct if the method is ever implemented.
  """
  @spec fetch_syncing_status(EthereumJSONRPC.json_rpc_named_arguments()) ::
          {:ok, :synced}
          | {:ok, {:syncing, current_block :: non_neg_integer(), highest_block :: non_neg_integer()}}
          | {:error, reason :: term()}
  def fetch_syncing_status(json_rpc_named_arguments) do
    %{id: 0, method: "eth_syncing", params: []}
    |> request()
    |> json_rpc(json_rpc_named_arguments)
    |> case do
      {:ok, false} ->
        {:ok, :synced}

      {:ok, %{"currentBlock" => current_block, "highestBlock" => highest_block}} ->
        {:ok, {:syncing, quantity_to_integer(current_block), quantity_to_integer(highest_block)}}

      {:error, _reason} ->
        {:ok, :synced}

      {:ok, other} ->
        {:error, {:unexpected_syncing_result, other}}
    end
  end

  @doc """
  Decimals the native coin is presented with.

  Defaults to 18, the EVM-facing figure every balance the indexer reads is
  denominated in. Configurable with `config :ethereum_jsonrpc,
  EthereumJSONRPC.PaxeerX, native_coin_decimals: integer`.
  """
  @spec native_coin_decimals() :: non_neg_integer()
  def native_coin_decimals do
    case config(:native_coin_decimals) do
      decimals when is_integer(decimals) and decimals >= 0 -> decimals
      _ -> @default_native_coin_decimals
    end
  end

  @doc """
  Decimals of the chain's bank denomination, `uhpx`: 6.
  """
  @spec bank_denom_decimals() :: non_neg_integer()
  def bank_denom_decimals, do: @bank_denom_decimals

  @doc """
  The factor the EVM state layer multiplies bank amounts by to present `uhpx` as
  an 18-decimal coin: 10^12.
  """
  @spec bank_denom_scaling_factor() :: pos_integer()
  def bank_denom_scaling_factor, do: Integer.pow(10, @evm_denom_decimals - @bank_denom_decimals)

  @doc """
  Converts a bank-side `uhpx` amount into the wei the EVM reports for it.
  """
  @spec bank_amount_to_wei(non_neg_integer()) :: non_neg_integer()
  def bank_amount_to_wei(amount) when is_integer(amount) and amount >= 0,
    do: amount * bank_denom_scaling_factor()

  @doc """
  Converts wei back to the bank-side `uhpx` amount, discarding the remainder the
  bank denomination cannot represent.
  """
  @spec wei_to_bank_amount(non_neg_integer()) :: non_neg_integer()
  def wei_to_bank_amount(wei) when is_integer(wei) and wei >= 0,
    do: div(wei, bank_denom_scaling_factor())

  defp new_head_number(new_head) do
    case new_head["number"] do
      nil -> {:error, {:invalid_new_head, :number}}
      quantity -> ok_or_invalid(quantity_to_integer(quantity), :number)
    end
  end

  defp new_head_hash(new_head) do
    case new_head["hash"] do
      hash when is_binary(hash) -> {:ok, hash}
      _ -> {:error, {:invalid_new_head, :hash}}
    end
  end

  defp ok_or_invalid(nil, field), do: {:error, {:invalid_new_head, field}}
  defp ok_or_invalid(value, _field), do: {:ok, value}

  defp hashes_equal?(left, right) when is_binary(left) and is_binary(right),
    do: String.downcase(left) == String.downcase(right)

  defp hashes_equal?(_left, _right), do: false

  defp config(key) do
    :ethereum_jsonrpc
    |> Application.get_env(__MODULE__, [])
    |> Keyword.get(key)
  end
end
