defmodule Explorer.Chain.PaxeerX.Finality.CapabilitySource do
  @moduledoc """
  The contract `Explorer.Chain.PaxeerX.Finality` uses to ask whether a Paxeer X
  Network precompile capability is live on the connected node.

  The production implementation is `Explorer.Chain.PaxeerX.Capabilities`; it is
  reached through this behaviour, and through the `:capability_source` key of
  `config :explorer, Explorer.Chain.PaxeerX.Finality`, so that a caller can
  supply another implementation and so that the ladder degrades to its database
  source when the capability module is not loaded.
  """

  @callback live?(capability :: atom()) :: boolean()
end

defmodule Explorer.Chain.PaxeerX.Finality.Anchor do
  @moduledoc """
  One anchor observation: the latest checkpoint the anchor reports, with the
  heights it covers and the source it was read from.

  * `:source` - `:anchor_table` for a row of `lx_anchors`, `:anchor_call` for a
    live read of the anchor precompile.
  * `:batch_number` - the anchor batch, known only on the live read.
  * `:checkpoint_height` - the chain height the checkpoint itself records.
  * `:sealed_height` - the highest chain height the checkpoint seals.
  * `:finalized_height` - the highest chain height covered by a checkpoint the
    anchor reports as finalized, `nil` when the anchor reports none. Never a
    substituted zero.
  * `:block_number` - the block the anchor row was observed in, known only on
    the database read.
  """

  @type t :: %__MODULE__{
          source: :anchor_table | :anchor_call,
          batch_number: non_neg_integer() | nil,
          checkpoint_height: non_neg_integer() | nil,
          sealed_height: non_neg_integer() | nil,
          finalized_height: non_neg_integer() | nil,
          block_number: non_neg_integer() | nil
        }

  @enforce_keys [:source]
  defstruct [
    :source,
    :batch_number,
    :checkpoint_height,
    :sealed_height,
    :finalized_height,
    :block_number
  ]
end

defmodule Explorer.Chain.PaxeerX.Finality.Heights do
  @moduledoc """
  The settlement heights `Explorer.Chain.PaxeerX.Status` decides a rung from.

  Every field is either a height or `nil`; a `nil` height means the rung it
  governs cannot be reached, never that it is zero.
  """

  alias Explorer.Chain.PaxeerX.Finality.Anchor

  @type sealed_source :: :anchor_table | :anchor_call | :no_anchor | {:error, term()}
  @type final_source :: :anchor | :confirmations | :none

  @type t :: %__MODULE__{
          instant_height: non_neg_integer() | nil,
          sealed_height: non_neg_integer() | nil,
          sealed_source: sealed_source(),
          final_height: non_neg_integer() | nil,
          final_source: final_source(),
          anchor: Anchor.t() | nil
        }

  defstruct instant_height: nil,
            sealed_height: nil,
            sealed_source: :no_anchor,
            final_height: nil,
            final_source: :none,
            anchor: nil
end

defmodule Explorer.Chain.PaxeerX.Finality.Cache do
  @moduledoc """
  Owner of the ETS table that holds the live anchor read for
  `PAXEER_X_FINALITY_CACHE_MS`.

  The table is public, so readers and writers never go through this process; it
  exists only to keep the table alive across the request processes that use it.
  It is started on demand by `Explorer.Chain.PaxeerX.Finality`, so the ladder
  works whether or not the surface using it runs under the explorer supervision
  tree.
  """

  use GenServer

  @table :paxeer_x_finality_cache

  @doc """
  The name of the public ETS table this process owns.
  """
  @spec table() :: atom()
  def table, do: @table

  @doc """
  Starts the table owner, returning the running one when it already exists.
  """
  @spec ensure_started() :: pid()
  def ensure_started do
    case Process.whereis(__MODULE__) do
      nil ->
        case GenServer.start(__MODULE__, :ok, name: __MODULE__) do
          {:ok, pid} -> pid
          {:error, {:already_started, pid}} -> pid
        end

      pid ->
        pid
    end
  end

  @spec start_link(term()) :: GenServer.on_start()
  def start_link(_options), do: GenServer.start_link(__MODULE__, :ok, name: __MODULE__)

  @impl GenServer
  def init(:ok) do
    :ets.new(@table, [:named_table, :public, :set, read_concurrency: true, write_concurrency: true])

    {:ok, %{}}
  end
end

defmodule Explorer.Chain.PaxeerX.Finality do
  @moduledoc """
  The height sources behind the Paxeer X Network settlement ladder.

  `Explorer.Chain.PaxeerX.Status` is a pure function over
  `t:Explorer.Chain.PaxeerX.Finality.Heights.t/0`; every database read, node
  read and configuration read the ladder needs happens here. The instant height
  is the highest consensus block the explorer has indexed. The sealed height
  comes from the anchor: from the latest `lx_anchors` row, or, when the anchor
  capability is live, from a read of the anchor precompile cached for
  `PAXEER_X_FINALITY_CACHE_MS` milliseconds. The final height comes from the
  same anchor read when it reports a finalized checkpoint, and otherwise from
  the `PAXEER_X_FINAL_CONFIRMATIONS` fallback, which is disabled at its default
  of zero. A height that no source reports stays `nil` and its rung stays out of
  reach rather than being stood in for by a zero.
  """

  alias Explorer.Chain.Cache.BlockNumber
  alias Explorer.Chain.PaxeerX.Finality.{Anchor, Cache, Heights}
  alias Explorer.Repo

  @anchor_precompile "0x0000000000000000000000000000000000001014"

  @anchor_abi [
    %{
      "type" => "function",
      "name" => "latestFinalized",
      "stateMutability" => "view",
      "inputs" => [],
      "outputs" => [
        %{"name" => "batchNumber", "type" => "uint64", "internalType" => "uint64"},
        %{"name" => "exists", "type" => "bool", "internalType" => "bool"}
      ]
    },
    %{
      "type" => "function",
      "name" => "statusOf",
      "stateMutability" => "view",
      "inputs" => [%{"name" => "batchNumber", "type" => "uint64", "internalType" => "uint64"}],
      "outputs" => [%{"name" => "", "type" => "uint8", "internalType" => "uint8"}]
    },
    %{
      "type" => "function",
      "name" => "checkpoint",
      "stateMutability" => "view",
      "inputs" => [%{"name" => "batchNumber", "type" => "uint64", "internalType" => "uint64"}],
      "outputs" => [
        %{
          "name" => "",
          "type" => "tuple",
          "internalType" => "struct ILayerXAnchor.Checkpoint",
          "components" => [
            %{"name" => "batchNumber", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "checkpointId", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "headerDigest", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "epoch", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "firstSequence", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "lastSequence", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "previousStateRoot", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "stateRoot", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "receiptRoot", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "dataAvailabilityRoot", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "sequencerId", "type" => "bytes32", "internalType" => "bytes32"},
            %{"name" => "timestampMs", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "status", "type" => "uint8", "internalType" => "uint8"},
            %{"name" => "signers", "type" => "uint8", "internalType" => "uint8"},
            %{"name" => "availabilityMask", "type" => "uint8", "internalType" => "uint8"},
            %{"name" => "openChallenges", "type" => "uint32", "internalType" => "uint32"},
            %{"name" => "submittedHeight", "type" => "uint64", "internalType" => "uint64"},
            %{"name" => "finalizedHeight", "type" => "uint64", "internalType" => "uint64"}
          ]
        }
      ]
    }
  ]

  @checkpoint_fields 18
  @checkpoint_submitted_height_index 16

  @latest_anchor_sql """
  SELECT block_number, checkpoint_height, sealed_height
  FROM lx_anchors
  ORDER BY sealed_height DESC, block_number DESC
  LIMIT 1
  """

  @missing_relation_codes [:undefined_table, :undefined_column]

  @default_capability_source Explorer.Chain.PaxeerX.Capabilities
  @default_cache_ms 1_000
  @default_probe_batches 16
  @default_final_confirmations 0

  @doc """
  Collects the heights `Explorer.Chain.PaxeerX.Status` decides a rung from.

  Options override a single source, so a caller that already holds a height does
  not pay for it twice:

  * `:instant_height` - the highest consensus block number.
  * `:anchor` - an already read `t:Explorer.Chain.PaxeerX.Finality.Anchor.t/0`,
    `:no_anchor`, or `{:error, reason}`.
  * `:json_rpc_named_arguments` - the node the live anchor read goes to.
  """
  @spec heights(keyword()) :: Heights.t()
  def heights(options \\ []) do
    instant_height = Keyword.get_lazy(options, :instant_height, &instant_height/0)
    anchor_read = Keyword.get_lazy(options, :anchor, fn -> read_anchor(options) end)

    {sealed_height, sealed_source, anchor} =
      case anchor_read do
        {:ok, %Anchor{} = anchor} -> {anchor.sealed_height, anchor.source, anchor}
        %Anchor{} = anchor -> {anchor.sealed_height, anchor.source, anchor}
        :no_anchor -> {nil, :no_anchor, nil}
        {:error, reason} -> {nil, {:error, reason}, nil}
      end

    {final_height, final_source} = final_height(anchor, instant_height)

    %Heights{
      instant_height: instant_height,
      sealed_height: sealed_height,
      sealed_source: sealed_source,
      final_height: final_height,
      final_source: final_source,
      anchor: anchor
    }
  end

  @doc """
  The highest consensus block number the explorer has indexed.
  """
  @spec instant_height() :: non_neg_integer()
  def instant_height, do: BlockNumber.get_max()

  @doc """
  The anchor the sealed rung is measured against: the live precompile read when
  the anchor capability is live, the latest `lx_anchors` row otherwise.
  """
  @spec read_anchor(keyword()) :: {:ok, Anchor.t()} | :no_anchor | {:error, term()}
  def read_anchor(options \\ []) do
    if anchor_capability_live?() do
      live_anchor(options)
    else
      latest_anchor()
    end
  end

  @doc """
  The latest anchor recorded in `lx_anchors`.

  The table is created by the Paxeer X migration that owns the `lx_*` tables; on
  a database where that migration has not run yet this returns `:no_anchor`
  rather than raising, so the ladder answers `:instant` instead of failing.
  """
  @spec latest_anchor() :: {:ok, Anchor.t()} | :no_anchor | {:error, term()}
  def latest_anchor do
    case Repo.query(@latest_anchor_sql, []) do
      {:ok, %Postgrex.Result{rows: [[block_number, checkpoint_height, sealed_height]]}} ->
        {:ok,
         %Anchor{
           source: :anchor_table,
           batch_number: nil,
           checkpoint_height: to_height(checkpoint_height),
           sealed_height: to_height(sealed_height),
           finalized_height: nil,
           block_number: to_height(block_number)
         }}

      {:ok, %Postgrex.Result{rows: []}} ->
        :no_anchor

      {:error, %Postgrex.Error{postgres: %{code: code}}} when code in @missing_relation_codes ->
        :no_anchor

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc """
  The latest anchor read from the anchor precompile, cached for
  `PAXEER_X_FINALITY_CACHE_MS` milliseconds.

  The anchor exposes the latest finalized batch directly; the latest sealed
  batch is the highest batch above it that the anchor still reports a status
  for, which is probed over `PAXEER_X_FINALITY_PROBE_BATCHES` batches in one
  batched `eth_call`. Both batches are then read as checkpoints, and the height
  each one seals is the height at which it was submitted.
  """
  @spec live_anchor(keyword()) :: {:ok, Anchor.t()} | :no_anchor | {:error, term()}
  def live_anchor(options \\ []) do
    json_rpc_named_arguments = Keyword.get_lazy(options, :json_rpc_named_arguments, &json_rpc_named_arguments/0)

    cached(:live_anchor, cache_ms(), fn -> read_live_anchor(json_rpc_named_arguments) end)
  end

  @doc """
  Whether the anchor precompile is live on the connected node.

  The capability module is reached through
  `Explorer.Chain.PaxeerX.Finality.CapabilitySource`; a source that is not
  loaded means the capability is not live, and the sealed rung falls to the
  `lx_anchors` source.
  """
  @spec anchor_capability_live?() :: boolean()
  def anchor_capability_live? do
    module = capability_source()

    if Code.ensure_loaded?(module) and function_exported?(module, :live?, 1) do
      module.live?(:anchor) == true
    else
      false
    end
  end

  @doc """
  The implementation of `Explorer.Chain.PaxeerX.Finality.CapabilitySource` the
  ladder asks about the anchor capability.
  """
  @spec capability_source() :: module()
  def capability_source, do: configured(:capability_source, @default_capability_source)

  @doc """
  How long a live anchor read is reused, in milliseconds.
  """
  @spec cache_ms() :: non_neg_integer()
  def cache_ms, do: configured(:cache_ms, @default_cache_ms)

  @doc """
  How many batches above the latest finalized one the live read probes for the
  latest sealed batch.
  """
  @spec probe_batches() :: non_neg_integer()
  def probe_batches, do: configured(:probe_batches, @default_probe_batches)

  @doc """
  The confirmation depth the final rung falls back to when the anchor reports no
  finalized checkpoint. Zero, the default, means the anchor is the only source
  of the final rung.
  """
  @spec final_confirmations() :: non_neg_integer()
  def final_confirmations, do: configured(:final_confirmations, @default_final_confirmations)

  @doc """
  Drops the cached live anchor read.
  """
  @spec clear_cache() :: :ok
  def clear_cache do
    Cache.ensure_started()
    :ets.delete(Cache.table(), :live_anchor)

    :ok
  end

  defp final_height(%Anchor{finalized_height: finalized_height}, _instant_height) when is_integer(finalized_height),
    do: {finalized_height, :anchor}

  defp final_height(_anchor, instant_height) do
    confirmations = final_confirmations()

    if confirmations > 0 and is_integer(instant_height) do
      {max(instant_height - confirmations, 0), :confirmations}
    else
      {nil, :none}
    end
  end

  defp read_live_anchor(json_rpc_named_arguments) do
    with {:ok, [batch_number, exists?]} <- call(:latest_finalized, [], json_rpc_named_arguments),
         finalized_batch = exists? && batch_number,
         sealed_batch = probe_sealed_batch(finalized_batch, json_rpc_named_arguments) do
      build_live_anchor(finalized_batch, sealed_batch, json_rpc_named_arguments)
    else
      {:ok, _other} -> {:error, :malformed_latest_finalized}
      {:error, reason} -> {:error, reason}
    end
  end

  defp build_live_anchor(false, nil, _json_rpc_named_arguments), do: :no_anchor

  defp build_live_anchor(finalized_batch, sealed_batch, json_rpc_named_arguments) do
    with {:ok, finalized_height} <- sealed_by(finalized_batch, json_rpc_named_arguments),
         {:ok, sealed_height} <- sealed_by(sealed_batch, json_rpc_named_arguments) do
      sealed_height = sealed_height || finalized_height

      {:ok,
       %Anchor{
         source: :anchor_call,
         batch_number: sealed_batch || finalized_batch || nil,
         checkpoint_height: sealed_height,
         sealed_height: sealed_height,
         finalized_height: finalized_height,
         block_number: nil
       }}
    end
  end

  defp sealed_by(batch, _json_rpc_named_arguments) when batch in [nil, false], do: {:ok, nil}

  defp sealed_by(batch, json_rpc_named_arguments) do
    case call(:checkpoint, [batch], json_rpc_named_arguments) do
      {:ok, [checkpoint]} when is_tuple(checkpoint) and tuple_size(checkpoint) == @checkpoint_fields ->
        {:ok, elem(checkpoint, @checkpoint_submitted_height_index)}

      {:ok, _other} ->
        {:error, :malformed_checkpoint}

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp probe_sealed_batch(finalized_batch, json_rpc_named_arguments) do
    first = if finalized_batch, do: finalized_batch + 1, else: 0
    probe = probe_batches()

    if probe > 0 do
      batches = Enum.to_list(first..(first + probe - 1)//1)

      batches
      |> Enum.map(&%{contract_address: @anchor_precompile, method_id: method_id(:status_of), args: [&1]})
      |> EthereumJSONRPC.execute_contract_functions(@anchor_abi, json_rpc_named_arguments)
      |> Enum.zip(batches)
      |> Enum.reduce_while(nil, fn
        {{:ok, [status]}, batch}, _highest when is_integer(status) and status > 0 -> {:cont, batch}
        _result, highest -> {:halt, highest}
      end)
    end
  end

  defp call(function, args, json_rpc_named_arguments) do
    [%{contract_address: @anchor_precompile, method_id: method_id(function), args: args}]
    |> EthereumJSONRPC.execute_contract_functions(@anchor_abi, json_rpc_named_arguments)
    |> List.first()
  end

  defp method_id(:latest_finalized), do: selector("latestFinalized()")
  defp method_id(:status_of), do: selector("statusOf(uint64)")
  defp method_id(:checkpoint), do: selector("checkpoint(uint64)")

  defp selector(signature) do
    signature
    |> ExKeccak.hash_256()
    |> binary_part(0, 4)
    |> Base.encode16(case: :lower)
  end

  defp cached(key, ttl_ms, read) do
    Cache.ensure_started()
    now = System.monotonic_time(:millisecond)

    case :ets.lookup(Cache.table(), key) do
      [{^key, expires_at, value}] when expires_at > now ->
        value

      _miss ->
        value = read.()
        :ets.insert(Cache.table(), {key, now + ttl_ms, value})

        value
    end
  end

  defp json_rpc_named_arguments, do: Application.get_env(:explorer, :json_rpc_named_arguments)

  defp configured(key, default) do
    :explorer
    |> Application.get_env(__MODULE__, [])
    |> Keyword.get(key, default)
  end

  defp to_height(nil), do: nil
  defp to_height(%Decimal{} = value), do: Decimal.to_integer(value)
  defp to_height(value) when is_integer(value), do: value
end
