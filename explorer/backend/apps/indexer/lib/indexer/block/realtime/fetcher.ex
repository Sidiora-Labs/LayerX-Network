defmodule Indexer.Block.Realtime.Fetcher do
  @moduledoc """
  Fetches and indexes block ranges from latest block forward using a WebSocket.
  """

  use GenServer
  use Spandex.Decorators

  require Indexer.Tracer
  require Logger

  import EthereumJSONRPC, only: [quantity_to_integer: 1]

  import Indexer.Block.Fetcher,
    only: [
      async_import_blobs: 2,
      async_import_block_rewards: 2,
      async_import_celo_epoch_block_operations: 2,
      async_import_celo_accounts: 2,
      async_import_created_contract_codes: 2,
      async_import_filecoin_addresses_info: 2,
      async_import_internal_transactions: 2,
      async_import_polygon_zkevm_bridge_l1_tokens: 1,
      async_import_realtime_coin_balances: 1,
      async_import_replaced_transactions: 2,
      async_import_signed_authorizations_statuses: 2,
      async_import_token_balances: 2,
      async_import_current_token_balances: 2,
      async_import_token_instances: 1,
      async_import_tokens: 2,
      async_import_uncles: 2,
      fetch_and_import_range: 2
    ]

  alias Ecto.Changeset
  alias EthereumJSONRPC.{Blocks, Subscription}
  alias Explorer.Chain
  alias Explorer.Chain.Cache.Counters.AverageBlockTime
  alias Explorer.Chain.Events.Publisher
  alias Explorer.Utility.MissingBlockRange
  alias Indexer.{Block, Tracer}
  alias Indexer.Block.Realtime.TaskSupervisor
  alias Indexer.Fetcher.OnDemand.ContractCreator, as: ContractCreatorOnDemand
  alias Indexer.Fetcher.Optimism
  alias Indexer.Prometheus
  alias Indexer.Prometheus.Instrumenter
  alias Timex.Duration

  @behaviour Block.Fetcher

  @minimum_safe_polling_period :timer.seconds(1)
  @max_realtime_blocks_in_memory 10

  @default_batch_size 64
  @default_max_lag 5_000

  @shutdown_after :timer.minutes(1)

  @enforce_keys ~w(block_fetcher)a

  defstruct block_fetcher: nil,
            subscription: nil,
            previous_number: nil,
            timer: nil,
            last_realtime_blocks: %{}

  @type t :: %__MODULE__{
          block_fetcher: Block.Fetcher.t(__MODULE__),
          subscription: Subscription.t(),
          previous_number: pos_integer() | nil,
          timer: reference(),
          last_realtime_blocks: map()
        }

  def start_link([arguments, gen_server_options]) do
    GenServer.start_link(__MODULE__, arguments, gen_server_options)
  end

  @impl GenServer
  def init(%{block_fetcher: %Block.Fetcher{} = block_fetcher, subscribe_named_arguments: subscribe_named_arguments}) do
    Logger.metadata(fetcher: :block_realtime)
    Process.flag(:trap_exit, true)

    {:ok, %__MODULE__{block_fetcher: %Block.Fetcher{block_fetcher | broadcast: :realtime, callback_module: __MODULE__}},
     {:continue, {:init, subscribe_named_arguments}}}
  end

  @impl GenServer
  def handle_continue({:init, subscribe_named_arguments}, %__MODULE__{subscription: nil} = state) do
    timer = schedule_polling()
    {:noreply, %__MODULE__{state | timer: timer} |> subscribe_to_new_heads(subscribe_named_arguments)}
  end

  # This handler catches blocks appeared in websocket (if WS is enabled).
  #
  # It takes into account the block hash which was lastly polled by the `:poll_latest_block_number` handler: if a block with
  # the same hash is already polled, the handler ignores it. Otherwise, the handler starts fetching and importing the block, and
  # schedules a new polling.
  #
  # ## Parameters
  # - `{subscription, {:ok, block}}` where `subscription` is an instance of WS subscription.
  # - `state` contains service parameters, also including:
  #   `previous_number` - the block number reported by the previous call.
  #   `timer` - the timer to call `:poll_latest_block_number` handler next time.
  #   `last_realtime_blocks` - a map of recent realtime blocks got earlier.
  #
  # ## Returns
  # - `{:noreply, state}` tuple where the `state` is the current or updated GenServer's state.
  @impl GenServer
  def handle_info(
        {subscription, {:ok, %{"number" => quantity, "hash" => hash} = new_head}},
        %__MODULE__{
          block_fetcher: %Block.Fetcher{} = block_fetcher,
          subscription: %Subscription{} = subscription,
          previous_number: previous_number,
          timer: timer,
          last_realtime_blocks: last_realtime_blocks
        } = state
      )
      when is_binary(quantity) do
    number = quantity_to_integer(quantity)

    if number > 0 do
      Publisher.broadcast([{:last_block_number, number}], :realtime)
    end

    if hash != Map.get(last_realtime_blocks, number) do
      Process.cancel_timer(timer)

      # Subscriptions don't support getting all the blocks and transactions data,
      # so we need to go back and get the full blocks of the whole range up to the new head
      start_fetch_and_import(
        number,
        block_fetcher,
        previous_number,
        parent_mismatch?(new_head, number, last_realtime_blocks)
      )

      new_timer = schedule_polling()

      {:noreply,
       %{
         state
         | previous_number: number,
           timer: new_timer,
           last_realtime_blocks: update_last_realtime_blocks(last_realtime_blocks, number, hash)
       }}
    else
      # the block must be ignored if this block was already got earlier.
      {:noreply, state}
    end
  end

  # This handler gets the latest block using RPC request to get the latest block number.
  # It fetches the latest block number, starts its fetching and importing, and schedules the next polling iteration.
  #
  # ## Parameters
  # - `:poll_latest_block_number`: The message that triggers the fetching process.
  # - `state` contains service parameters, also including:
  #   `previous_number` - the block number reported by the previous block number fetching.
  #   `timer` - the timer to call this handler next time.
  #   `last_realtime_blocks` - a map of recent realtime blocks got earlier.
  #
  # ## Returns
  # - `{:noreply, state}` tuple where the `state` is the current or updated GenServer's state.
  #   Contains updated `previous_number`, `timer`, and `last_realtime_blocks` values.
  @impl GenServer
  def handle_info(
        :poll_latest_block_number,
        %__MODULE__{
          block_fetcher: %Block.Fetcher{json_rpc_named_arguments: json_rpc_named_arguments} = block_fetcher,
          previous_number: previous_number,
          last_realtime_blocks: last_realtime_blocks
        } = state
      ) do
    {new_previous_number, new_last_realtime_blocks} =
      with {:ok, %Blocks{blocks_params: [%{number: number, hash: hash}]}} <-
             EthereumJSONRPC.fetch_block_by_tag("latest", json_rpc_named_arguments),
           {:new_block, true, _} <- {:new_block, hash != last_realtime_blocks[number], number} do
        number =
          if abnormal_gap?(number, previous_number) do
            new_number = max(number, previous_number)
            start_fetch_and_import(new_number, block_fetcher, previous_number)
            new_number
          else
            start_fetch_and_import(number, block_fetcher, previous_number)
            number
          end

        fetch_validators_async()
        {number, update_last_realtime_blocks(last_realtime_blocks, number, hash)}
      else
        _ ->
          # the block must be ignored if this block was already got earlier.
          {previous_number, last_realtime_blocks}
      end

    timer = schedule_polling()

    {:noreply,
     %{
       state
       | previous_number: new_previous_number,
         timer: timer,
         last_realtime_blocks: new_last_realtime_blocks
     }}
  end

  # don't handle other messages (e.g. :ssl_closed)
  def handle_info(_, state) do
    {:noreply, state}
  end

  @spec update_last_realtime_blocks(map(), non_neg_integer(), binary()) :: map()
  defp update_last_realtime_blocks(last_realtime_blocks, number, hash) do
    last_realtime_blocks
    |> Enum.reject(fn {n, _} -> n <= number - @max_realtime_blocks_in_memory end)
    |> Enum.into(%{})
    |> Map.put(number, hash)
  end

  @impl GenServer
  def terminate(_reason, %__MODULE__{timer: timer}) do
    Process.cancel_timer(timer)
  end

  defp fetch_validators_async do
    chain_type = Application.get_env(:explorer, :chain_type)
    do_fetch_validators_async(chain_type)
  end

  defp do_fetch_validators_async(:stability) do
    alias Indexer.Fetcher.Stability.Validator, as: StabilityValidator

    StabilityValidator.trigger_update_validators_list()
  end

  defp do_fetch_validators_async(:blackfort) do
    alias Indexer.Fetcher.Blackfort.Validator, as: BlackfortValidator

    BlackfortValidator.trigger_update_validators_list()
  end

  defp do_fetch_validators_async(_chain_type) do
    :ignore
  end

  defp subscribe_to_new_heads(%__MODULE__{subscription: nil} = state, subscribe_named_arguments)
       when is_list(subscribe_named_arguments) do
    case EthereumJSONRPC.subscribe("newHeads", subscribe_named_arguments) do
      {:ok, subscription} ->
        %__MODULE__{state | subscription: subscription}

      {:error, reason} ->
        Logger.debug(fn -> ["Could not connect to websocket: #{inspect(reason)}. Continuing with polling."] end)
        state
    end
  catch
    :exit, _reason ->
      if Map.get(state, :timer) && state.timer do
        Process.cancel_timer(state.timer)
      end

      timer = schedule_polling()
      %{state | timer: timer}
  end

  defp subscribe_to_new_heads(state, _), do: state

  defp schedule_polling do
    polling_period =
      case Application.get_env(:indexer, __MODULE__)[:polling_period] do
        nil ->
          case AverageBlockTime.average_block_time() do
            {:error, :disabled} -> 2_000
            block_time -> min(round(Duration.to_milliseconds(block_time) / 2), 30_000)
          end

        period ->
          period
      end

    safe_polling_period = max(polling_period, @minimum_safe_polling_period)

    Process.send_after(self(), :poll_latest_block_number, safe_polling_period)
  end

  @import_options ~w(address_hash_to_fetched_balance_block_number)a

  @impl Block.Fetcher
  def import(_block_fetcher, %{block_rewards: block_rewards} = options) do
    {block_reward_errors, chain_import_block_rewards} = Map.pop(block_rewards, :errors)

    chain_import_options =
      options
      |> Map.drop(@import_options)
      |> put_in([:blocks, :params, Access.all(), :consensus], true)
      |> put_in([:blocks, :params, Access.all(), :refetch_needed], false)
      |> put_in([:block_rewards], chain_import_block_rewards)

    with {:import, {:ok, imported} = ok} <- {:import, Chain.import(chain_import_options)} do
      last_block =
        chain_import_options[:blocks][:params]
        |> Enum.max_by(& &1.number, fn -> nil end)

      if not is_nil(last_block) do
        Instrumenter.set_latest_block(last_block.number, last_block.timestamp)
      end

      async_import_remaining_block_data(
        imported,
        %{block_rewards: %{errors: block_reward_errors}}
      )

      ContractCreatorOnDemand.async_update_cache_of_contract_creator_on_demand(imported)

      ok
    end
  end

  def import(_, _) do
    Logger.warning("Empty parameters were provided for realtime fetcher")

    {:ok, []}
  end

  @doc """
  Fetches and imports every block from the last known height up to `number` as batched ranges.

  The range is split into chunks of at most `INDEXER_REALTIME_BATCH_SIZE` blocks and each chunk is imported with a
  single `Indexer.Block.Fetcher.fetch_and_import_range/2` call, so that blocks, receipts and the coin balances of the
  addresses touched anywhere in the chunk are fetched with one set of batched JSON RPC requests instead of one set per
  block.

  When the distance between `previous_number` and `number` is larger than `INDEXER_REALTIME_MAX_LAG`, the older part of
  the range is handed to the catchup fetcher through `Explorer.Utility.MissingBlockRange` instead of blocking the
  realtime fetcher with the backlog.
  """
  @spec start_fetch_and_import(non_neg_integer(), Block.Fetcher.t(module()), non_neg_integer() | nil, boolean()) :: :ok
  def start_fetch_and_import(number, block_fetcher, previous_number, parent_mismatch? \\ false) do
    is_reorg = reorg?(number, previous_number) or parent_mismatch?

    number
    |> determine_start_at(previous_number, parent_mismatch?)
    |> import_ranges(number)
    |> Enum.each(fn range ->
      args = [range, block_fetcher, is_reorg]

      Task.Supervisor.start_child(TaskSupervisor, __MODULE__, :fetch_and_import_block_range, args,
        shutdown: @shutdown_after
      )
    end)
  end

  # `true` when the new head reports a parent hash that differs from the hash already seen for `number - 1`, which means
  # the previously imported parent has to be refetched together with the new head.
  @spec parent_mismatch?(map(), non_neg_integer(), map()) :: boolean()
  defp parent_mismatch?(%{"parentHash" => parent_hash}, number, last_realtime_blocks) when is_binary(parent_hash) do
    case Map.get(last_realtime_blocks, number - 1) do
      nil -> false
      known_parent_hash -> known_parent_hash != parent_hash
    end
  end

  defp parent_mismatch?(_new_head, _number, _last_realtime_blocks), do: false

  defp determine_start_at(number, nil, _parent_mismatch?), do: number

  defp determine_start_at(number, previous_number, parent_mismatch?) do
    cond do
      reorg?(number, previous_number) ->
        # set start_at to NOT fill in skipped numbers
        number

      parent_mismatch? ->
        # set start_at to also refetch the parent whose hash no longer matches the new head
        max(min(previous_number + 1, number - 1), 0)

      true ->
        # set start_at to fill in skipped numbers, if any
        previous_number + 1
    end
  end

  # Splits `start_at..number` into the ranges the realtime fetcher imports itself, handing the part that exceeds
  # `INDEXER_REALTIME_MAX_LAG` to the catchup fetcher.
  @spec import_ranges(non_neg_integer(), non_neg_integer()) :: [Range.t()]
  defp import_ranges(start_at, number) when start_at > number, do: []

  defp import_ranges(start_at, number) do
    max_lag = max_lag()

    realtime_start_at =
      if number - start_at + 1 > max_lag do
        hand_off_to_catchup(start_at..(number - max_lag)//1, max_lag)
        number - max_lag + 1
      else
        start_at
      end

    realtime_start_at..number//1
    |> Enum.chunk_every(batch_size())
    |> Enum.map(fn numbers -> List.first(numbers)..List.last(numbers)//1 end)
  end

  defp hand_off_to_catchup(first..last//_ = range, max_lag) do
    MissingBlockRange.save_batch([range])

    Logger.info(fn ->
      [
        "Realtime lag is above ",
        to_string(max_lag),
        " blocks, so blocks ",
        to_string(first),
        "..",
        to_string(last),
        " were handed to the catchup fetcher."
      ]
    end)
  end

  defp batch_size do
    max(Application.get_env(:indexer, __MODULE__)[:batch_size] || @default_batch_size, 1)
  end

  defp max_lag do
    max(Application.get_env(:indexer, __MODULE__)[:max_lag] || @default_max_lag, 1)
  end

  defp reorg?(number, previous_number) when is_integer(previous_number) and number <= previous_number do
    true
  end

  defp reorg?(_, _), do: false

  @default_max_gap 1000
  defp abnormal_gap?(_number, nil), do: false

  defp abnormal_gap?(number, previous_number) do
    max_gap = Application.get_env(:indexer, __MODULE__)[:max_gap] || @default_max_gap

    abs(number - previous_number) > max_gap
  end

  @reorg_delay 5_000

  @decorate trace(
              name: "fetch",
              resource: "Indexer.Block.Realtime.Fetcher.fetch_and_import_block_range/3",
              tracer: Tracer
            )
  def fetch_and_import_block_range(first..last//_ = range, block_fetcher, reorg?, retry \\ 3) do
    Process.flag(:trap_exit, true)

    Indexer.Logger.metadata(
      fn ->
        if reorg? do
          Enum.each(range, &remove_assets_by_number/1)

          # give previous fetch attempt (for the same block numbers) a chance to finish
          # before fetching again, to reduce block consensus mistakes
          :timer.sleep(@reorg_delay)
        end

        do_fetch_and_import_block_range(range, block_fetcher, retry)
      end,
      fetcher: :block_realtime,
      first_block_number: first,
      block_number: last
    )
  end

  def fetch_and_import_block(block_number_to_fetch, block_fetcher, reorg?, retry \\ 3) do
    fetch_and_import_block_range(block_number_to_fetch..block_number_to_fetch//1, block_fetcher, reorg?, retry)
  end

  @spec remove_assets_by_number(non_neg_integer()) :: any()
  defp remove_assets_by_number(reorg_block) do
    chain_type = Application.get_env(:explorer, :chain_type)
    do_remove_assets_by_number(chain_type, reorg_block)
  end

  # Removes all rows from `op_transaction_batches`, `op_withdrawals`,
  # `op_eip1559_config_updates`, and `op_interop_messages` tables
  # previously written starting from the reorg block number
  defp do_remove_assets_by_number(:optimism, reorg_block_number) do
    # credo:disable-for-lines:5 Credo.Check.Design.AliasUsage
    Optimism.handle_realtime_l2_reorg(reorg_block_number, Indexer.Fetcher.Optimism.EIP1559ConfigUpdate)
    Optimism.handle_realtime_l2_reorg(reorg_block_number, Indexer.Fetcher.Optimism.Interop.Message)
    Optimism.handle_realtime_l2_reorg(reorg_block_number, Indexer.Fetcher.Optimism.Interop.MessageFailed)
    Indexer.Fetcher.Optimism.TransactionBatch.handle_l2_reorg(reorg_block_number)
    Indexer.Fetcher.Optimism.Withdrawal.remove(reorg_block_number)
  end

  # Removes all rows from `polygon_zkevm_bridge` table
  # previously written starting from the reorg block number
  defp do_remove_assets_by_number(:polygon_zkevm, reorg_block) do
    # credo:disable-for-next-line Credo.Check.Design.AliasUsage
    Indexer.Fetcher.PolygonZkevm.BridgeL2.reorg_handle(reorg_block)
  end

  # Removes all rows from `shibarium_bridge` table
  # previously written starting from the reorg block number
  defp do_remove_assets_by_number(:shibarium, reorg_block) do
    # credo:disable-for-next-line Credo.Check.Design.AliasUsage
    Indexer.Fetcher.Shibarium.L2.reorg_handle(reorg_block)
  end

  # Removes all rows from `scroll_bridge` and `scroll_l1_fee_params` tables
  # previously written starting from the reorg block number
  defp do_remove_assets_by_number(:scroll, reorg_block) do
    # credo:disable-for-lines:2 Credo.Check.Design.AliasUsage
    Indexer.Fetcher.Scroll.BridgeL2.reorg_handle(reorg_block)
    Indexer.Fetcher.Scroll.L1FeeParam.handle_l2_reorg(reorg_block)
  end

  defp do_remove_assets_by_number(_, _), do: :ok

  @decorate span(tracer: Tracer)
  defp do_fetch_and_import_block_range(first..last//_ = range, block_fetcher, retry) do
    time_before = Timex.now()

    {fetch_duration, result} = :timer.tc(fn -> fetch_and_import_range(block_fetcher, range) end)

    Prometheus.Instrumenter.set_block_full_process(fetch_duration, __MODULE__)

    case result do
      {:ok, %{inserted: inserted, errors: []}} ->
        log_import_timings(inserted, fetch_duration, time_before)
        MissingBlockRange.clear_batch([range])
        Logger.debug("Fetched and imported.")

      {:ok, %{inserted: _, errors: [_ | _] = errors}} ->
        Logger.error(fn ->
          [
            "failed to fetch blocks ",
            to_string(first),
            "..",
            to_string(last),
            ": ",
            inspect(errors),
            ".  Blocks will be retried by catchup indexer."
          ]
        end)

      {:error, {:import = step, [%Changeset{} | _] = changesets}} ->
        Prometheus.Instrumenter.set_import_errors_count()

        params = %{
          changesets: changesets,
          range: range,
          block_fetcher: block_fetcher,
          retry: retry
        }

        if retry_fetch_and_import_block_range(params) == :ignore do
          Logger.error(
            fn ->
              [
                "failed to validate for blocks ",
                to_string(first),
                "..",
                to_string(last),
                ": ",
                inspect(changesets),
                ".  Blocks will be retried by catchup indexer."
              ]
            end,
            step: step
          )
        end

      {:error, {:import = step, reason}} ->
        Prometheus.Instrumenter.set_import_errors_count()
        Logger.error(fn -> inspect(reason) end, step: step)

      {:error, {step, reason}} ->
        Logger.error(
          fn ->
            [
              "failed to fetch: ",
              inspect(reason),
              ".  Blocks will be retried by catchup indexer."
            ]
          end,
          step: step
        )

      {:error, {step, failed_value, _changes_so_far}} ->
        Logger.error(
          fn ->
            [
              "failed to insert: ",
              inspect(failed_value),
              ".  Blocks will be retried by catchup indexer."
            ]
          end,
          step: step
        )
    end
  end

  defp log_import_timings(%{blocks: [_ | _] = blocks}, fetch_duration, time_before) do
    %{number: number, timestamp: timestamp} = Enum.max_by(blocks, & &1.number)

    node_delay = Timex.diff(time_before, timestamp, :seconds)
    Prometheus.Instrumenter.set_json_rpc_node_delay(node_delay)

    Logger.debug(
      "#{Enum.count(blocks)} block(s) up to #{number} fetching duration: #{fetch_duration / 1_000_000}s. Node delay: #{node_delay}s.",
      fetcher: :block_import_timings
    )
  end

  defp log_import_timings(_inserted, _duration, _time_before), do: nil

  defp retry_fetch_and_import_block_range(%{retry: retry}) when retry < 1, do: :ignore

  defp retry_fetch_and_import_block_range(%{changesets: changesets} = params) do
    if unknown_block_number_error?(changesets) do
      # Wait half a second to give the node time to sync.
      :timer.sleep(500)

      do_fetch_and_import_block_range(params.range, params.block_fetcher, params.retry - 1)
    else
      :ignore
    end
  end

  defp unknown_block_number_error?(changesets) do
    Enum.any?(changesets, &(Map.get(&1, :message) == "Unknown block number"))
  end

  defp async_import_remaining_block_data(
         imported,
         %{block_rewards: %{errors: block_reward_errors}}
       ) do
    realtime? = true

    async_import_realtime_coin_balances(imported)
    async_import_block_rewards(block_reward_errors, realtime?)
    async_import_created_contract_codes(imported, realtime?)
    async_import_internal_transactions(imported, realtime?)
    async_import_tokens(imported, realtime?)
    async_import_token_balances(imported, realtime?)
    async_import_current_token_balances(imported, realtime?)
    async_import_token_instances(imported)
    async_import_uncles(imported, realtime?)
    async_import_replaced_transactions(imported, realtime?)
    async_import_blobs(imported, realtime?)
    async_import_polygon_zkevm_bridge_l1_tokens(imported)
    async_import_celo_epoch_block_operations(imported, realtime?)
    async_import_celo_accounts(imported, realtime?)
    async_import_filecoin_addresses_info(imported, realtime?)
    async_import_signed_authorizations_statuses(imported, realtime?)
  end
end
