defmodule Explorer.Chain.PaxeerX.Capabilities do
  @moduledoc """
  Discovery of the Paxeer X Network surfaces the connected node answers today.

  The LayerX-side precompiles are not part of the chain that is live now: they
  arrive with a later upgrade, and until then `eth_getCode` at their addresses
  answers `0x`. Everything the explorer builds on top of them has to ask before
  it renders, so this process probes the node the same way the platform gateway
  does and publishes the answer to the rest of the application.

  One probe reads the node head with `eth_blockNumber` and then, at that one
  height, issues

    * `eth_getCode` for the custody (`0x…1013`), anchor (`0x…1014`), exchange
      (`0x…1015`), bridge (`0x…1016`) and launchpad (`0x…1017`) precompiles — a
      surface is live when the answer carries at least one byte of code;
    * one `eth_call` of `getUnifiedAccount(address)` (selector `0x357feed6`) at
      the addr precompile `0x…1004`, which exists before the upgrade but only
      answers that method after it — the capability is live when the call
      returns the declared four-word head instead of reverting.

  The first probe runs at startup, every following one after
  `PAXEER_X_CAPABILITIES_REFRESH_SECONDS` (30 by default). A probe that the node
  refuses leaves the previous answer in place and is retried on the next tick;
  it never replaces a known answer with a guess.

  ## API for other modules

  These four functions are the stable surface other parts of the explorer call:

    * `live?/1` — `true` when the capability was live at the last successful
      probe, `false` for every capability while the process is not running or
      has not answered yet. Takes `:custody`, `:anchor`, `:exchange`, `:bridge`,
      `:launchpad` or `:unified_account`.
    * `all/0` — the whole snapshot as a map of those six booleans plus
      `:checked_at`, the `DateTime` of the last successful probe or `nil`.
    * `refresh/0` — probes now, in the caller's stead, and returns the snapshot
      it stored; `{:error, reason}` when the node refuses.
    * `surfaces/0` — the capability names in their published order.

  A caller never has to check whether the process is running: a missing process
  reads as "nothing is live", which is what the chain answers today.
  """

  use GenServer

  require Logger

  import EthereumJSONRPC, only: [json_rpc: 2, request: 1]

  alias EthereumJSONRPC.Contract

  @table :paxeer_x_capabilities
  @snapshot_key :snapshot

  @custody_precompile "0x0000000000000000000000000000000000001013"
  @anchor_precompile "0x0000000000000000000000000000000000001014"
  @exchange_precompile "0x0000000000000000000000000000000000001015"
  @bridge_precompile "0x0000000000000000000000000000000000001016"
  @launchpad_precompile "0x0000000000000000000000000000000000001017"
  @addr_precompile "0x0000000000000000000000000000000000001004"

  @code_surfaces [
    custody: @custody_precompile,
    anchor: @anchor_precompile,
    exchange: @exchange_precompile,
    bridge: @bridge_precompile,
    launchpad: @launchpad_precompile
  ]

  @code_surface_names Keyword.keys(@code_surfaces)

  @surfaces [:unified_account | @code_surface_names]

  @get_unified_account_selector "0x357feed6"
  @probed_account "0x0000000000000000000000000000000000000000"

  # `(address evm, string paxAddr, bytes32 didPublicKey, bytes32 layerxMainAccountId)`
  # occupies four head words; a node that does not know the method answers
  # shorter than that or refuses outright.
  @unified_account_head_bytes 128

  @unified_account_request_id length(@code_surfaces)
  @head_request_id 0

  @default_refresh_interval_seconds 30

  @type surface :: :custody | :anchor | :exchange | :bridge | :launchpad | :unified_account

  @type snapshot :: %{
          custody: boolean(),
          anchor: boolean(),
          exchange: boolean(),
          bridge: boolean(),
          launchpad: boolean(),
          unified_account: boolean(),
          checked_at: DateTime.t() | nil
        }

  @spec start_link(term()) :: GenServer.on_start()
  def start_link(_) do
    GenServer.start_link(__MODULE__, [], name: __MODULE__)
  end

  @doc """
  The capability names in the order the API publishes them.
  """
  @spec surfaces() :: [surface()]
  def surfaces, do: @surfaces

  @doc """
  The precompile address a capability is probed at.
  """
  @spec address(surface()) :: String.t()
  def address(:unified_account), do: @addr_precompile

  def address(surface) when surface in @code_surface_names,
    do: Keyword.fetch!(@code_surfaces, surface)

  @doc """
  Whether `surface` was live at the last successful probe.

  `false` while no probe has succeeded, including when the process is not
  running at all, since that is what the chain answers before the upgrade.
  """
  @spec live?(surface()) :: boolean()
  def live?(surface) when surface in @surfaces do
    Map.fetch!(all(), surface)
  end

  @doc """
  The last successful probe, or the all-absent snapshot when there is none.
  """
  @spec all() :: snapshot()
  def all do
    case :ets.whereis(@table) do
      :undefined ->
        absent()

      table ->
        case :ets.lookup(table, @snapshot_key) do
          [{@snapshot_key, snapshot}] -> snapshot
          [] -> absent()
        end
    end
  end

  @doc """
  Probes the node now and returns the snapshot it stored.
  """
  @spec refresh() :: {:ok, snapshot()} | {:error, term()}
  def refresh do
    GenServer.call(__MODULE__, :refresh, :timer.seconds(30))
  end

  @doc """
  Reads the five surface precompiles and the addr method at one node height.

  Returns the snapshot the node describes, or the reason it refused. Exposed so
  a caller with its own JSON-RPC arguments can probe without this process.
  """
  @spec probe(EthereumJSONRPC.json_rpc_named_arguments()) :: {:ok, snapshot()} | {:error, term()}
  def probe(json_rpc_named_arguments) do
    with {:ok, block_number} <- fetch_head(json_rpc_named_arguments),
         {:ok, responses} <- json_rpc(probe_requests(block_number), json_rpc_named_arguments) do
      read(responses)
    end
  end

  @impl GenServer
  def init(_) do
    :ets.new(@table, [:named_table, :set, :protected, read_concurrency: true])

    {:ok, %{}, {:continue, :probe}}
  end

  @impl GenServer
  def handle_continue(:probe, state) do
    probe_and_store()
    schedule_refresh()

    {:noreply, state}
  end

  @impl GenServer
  def handle_info(:refresh, state) do
    probe_and_store()
    schedule_refresh()

    {:noreply, state}
  end

  @impl GenServer
  def handle_call(:refresh, _from, state) do
    {:reply, probe_and_store(), state}
  end

  defp probe_and_store do
    case probe(Application.get_env(:explorer, :json_rpc_named_arguments)) do
      {:ok, snapshot} ->
        :ets.insert(@table, {@snapshot_key, snapshot})

        {:ok, snapshot}

      {:error, reason} ->
        Logger.warning(fn ->
          [
            "Paxeer X capability probe failed, keeping the previous answer. Reason: ",
            inspect(reason)
          ]
        end)

        {:error, reason}
    end
  end

  defp schedule_refresh do
    Process.send_after(self(), :refresh, :timer.seconds(refresh_interval_seconds()))
  end

  defp refresh_interval_seconds do
    Application.get_env(:explorer, __MODULE__)[:refresh_interval_seconds] ||
      @default_refresh_interval_seconds
  end

  defp fetch_head(json_rpc_named_arguments) do
    %{id: @head_request_id, method: "eth_blockNumber", params: []}
    |> request()
    |> json_rpc(json_rpc_named_arguments)
    |> case do
      {:ok, quantity} ->
        case EthereumJSONRPC.quantity_to_integer(quantity) do
          number when is_integer(number) and number >= 0 -> {:ok, number}
          _ -> {:error, {:unexpected_head, quantity}}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp probe_requests(block_number) do
    block_quantity = EthereumJSONRPC.integer_to_quantity(block_number)

    code_requests =
      @code_surfaces
      |> Enum.with_index()
      |> Enum.map(fn {{_surface, address}, index} ->
        request(%{id: index, method: "eth_getCode", params: [address, block_quantity]})
      end)

    unified_account_request =
      Contract.eth_call_request(
        @get_unified_account_selector <> address_argument(@probed_account),
        @addr_precompile,
        @unified_account_request_id,
        block_number,
        nil
      )

    [unified_account_request | code_requests]
  end

  defp address_argument("0x" <> digits),
    do: String.duplicate("0", 64 - String.length(digits)) <> digits

  defp read(responses) do
    by_id = Map.new(responses, fn %{id: id} = response -> {id, response} end)

    with {:ok, code_surfaces} <- read_code_surfaces(by_id),
         {:ok, unified_account} <-
           read_unified_account(Map.get(by_id, @unified_account_request_id)) do
      {:ok,
       code_surfaces
       |> Map.put(:unified_account, unified_account)
       |> Map.put(:checked_at, DateTime.utc_now())}
    end
  end

  defp read_code_surfaces(by_id) do
    @code_surfaces
    |> Enum.with_index()
    |> Enum.reduce_while({:ok, %{}}, fn {{surface, _address}, index}, {:ok, acc} ->
      case read_code(Map.get(by_id, index)) do
        {:ok, live?} -> {:cont, {:ok, Map.put(acc, surface, live?)}}
        {:error, reason} -> {:halt, {:error, {surface, reason}}}
      end
    end)
  end

  defp read_code(%{result: "0x" <> digits}) do
    if rem(String.length(digits), 2) == 0 and hexadecimal?(digits) do
      {:ok, digits != ""}
    else
      {:error, {:unexpected_code, digits}}
    end
  end

  defp read_code(%{error: error}), do: {:error, error}

  defp read_code(other), do: {:error, {:unexpected_response, other}}

  # A node without the method answers the `eth_call` with an error or with
  # fewer bytes than the declared return layout; either way the capability is
  # absent rather than the probe being broken, so the surface reads as absent
  # and the rest of the snapshot still stands.
  defp read_unified_account(%{result: "0x" <> digits}) do
    live? =
      hexadecimal?(digits) and rem(String.length(digits), 2) == 0 and
        div(String.length(digits), 2) >= @unified_account_head_bytes

    {:ok, live?}
  end

  defp read_unified_account(%{error: _error}), do: {:ok, false}

  defp read_unified_account(other), do: {:error, {:unexpected_response, other}}

  defp hexadecimal?(digits) do
    String.match?(digits, ~r/\A[0-9a-fA-F]*\z/)
  end

  defp absent do
    @surfaces
    |> Map.new(&{&1, false})
    |> Map.put(:checked_at, nil)
  end
end
