defmodule EthereumJSONRPC.HTTP.Throttle do
  @moduledoc """
  Per-endpoint guard in front of the JSON-RPC HTTP transport.

  The indexer talks to a single full node, so an unbounded fan-out of archive-style requests is
  enough to take that node down. Every HTTP call made by `EthereumJSONRPC.HTTP` asks this process
  for a slot first and only then opens the connection.

  Four limits are applied per endpoint URL:

    * `:max_inflight` - how many JSON-RPC HTTP requests may be in flight at once;
    * `:rps` - a token bucket, refilled continuously, that caps requests per second. A batch costs
      one token per sub-request, clamped to the bucket capacity, so one large batch cannot consume
      more than a second of budget;
    * `:trace_max_inflight` - a lower concurrency cap for tracing calls (`debug_trace*`, `trace_*`);
    * `:archive_max_inflight` - a lower concurrency cap for historical state reads, that is
      `eth_getBalance`, `eth_getCode`, `eth_getStorageAt` and `eth_call` against a block other than
      `latest` or `pending`.

  A batch counts as a single in-flight request, and callers that cannot be admitted are queued in
  arrival order instead of being rejected: the guard replies to them once a slot frees up. The
  queue is scanned in order on every change, and an entry that cannot be admitted is skipped rather
  than blocking the ones behind it, so a saturated trace cap never stalls ordinary requests.

  Identical single requests that arrive while the same request is already in flight are coalesced:
  the later callers wait for the in-flight call and receive its response instead of opening another
  connection. Only the safelisted read methods in `@coalescable_methods` and tracing calls are
  coalesced, and only single requests, never batches. `EthereumJSONRPC.HTTP` reads `result`/`error`
  out of a single-request response and ignores the response `id`, so sharing one response between
  callers that used different ids is transparent to them.

  The guard is fail-closed. If this process is not running, callers wait for its supervisor to
  restart it and acquire a slot from the new process; no request is ever let through unmetered.

  Queue depth, in-flight counts and admission wait time are exported through
  `EthereumJSONRPC.HTTP.Throttle.Metrics`.
  """

  use GenServer

  require Logger

  alias EthereumJSONRPC.HTTP.Throttle.Metrics

  @classes [:default, :archive, :trace]

  @default_max_inflight 32
  @default_rps 200
  @default_trace_max_inflight 4
  @default_archive_max_inflight 8
  @default_coalesce? true

  # How long a caller waits before looking the guard up again after the guard went down. The caller
  # never proceeds without a slot, so a restart delays requests instead of releasing a burst.
  @unavailable_retry_interval 50

  # Upper bound on how long the guard sleeps before it re-examines a non-empty queue.
  @max_pump_interval 50

  @trace_method_prefixes ~w(debug_trace trace_)

  @historical_block_parameter_index %{
    "eth_call" => 1,
    "eth_getBalance" => 1,
    "eth_getCode" => 1,
    "eth_getStorageAt" => 2
  }

  @latest_block_parameters ~w(latest pending)

  @coalescable_methods ~w(
    eth_blockNumber
    eth_call
    eth_chainId
    eth_getBalance
    eth_getBlockByHash
    eth_getBlockByNumber
    eth_getBlockReceipts
    eth_getCode
    eth_getLogs
    eth_getStorageAt
    eth_getTransactionByHash
    eth_getTransactionCount
    eth_getTransactionReceipt
    eth_getUncleByBlockHashAndIndex
    eth_getUncleByBlockNumberAndIndex
    eth_syncing
  )

  @type class :: :default | :archive | :trace

  @type ticket :: %{
          url: String.t() | nil,
          class: class(),
          weight: pos_integer(),
          key: term(),
          ref: reference()
        }

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    {name, opts} = Keyword.pop(opts, :name, __MODULE__)

    GenServer.start_link(__MODULE__, opts, name: name)
  end

  @doc """
  Runs `fun` once a slot for `request` on `url` is available.

  `request` is the single JSON-RPC request map or the batch about to be sent, and `fun` is the
  function that performs the HTTP call. The return value of `fun` is returned unchanged. Callers
  wait for a slot; they are never rejected because the guard is busy or absent.
  """
  @spec request(String.t() | nil, map() | [map()], (-> result)) :: result when result: term()
  def request(url, request, fun) when is_function(fun, 0) do
    {class, weight} = classify(request)
    key = coalesce_key(url, request)

    case acquire(url, class, weight, key) do
      {:run, ticket} -> run(ticket, fun)
      {:coalesced, result} -> result
    end
  end

  @doc """
  Current in-flight counts and queue depth for `url`.
  """
  @spec stats(String.t() | nil) :: %{
          inflight: %{class() => non_neg_integer()},
          inflight_total: non_neg_integer(),
          queue_depth: non_neg_integer()
        }
  def stats(url) do
    GenServer.call(__MODULE__, {:stats, url})
  end

  @doc """
  Classifies `request` into the class whose cap applies to it and the number of rate budget tokens
  it costs.
  """
  @spec classify(map() | [map()]) :: {class(), pos_integer()}
  def classify(request) when is_map(request), do: {classify_method(request), 1}

  def classify(batch) when is_list(batch) do
    {class, weight} =
      Enum.reduce(batch, {:default, 0}, fn request, {class, weight} ->
        {heavier(class, classify_method(request)), weight + 1}
      end)

    {class, max(weight, 1)}
  end

  @impl GenServer
  def init(opts) do
    {:ok, %{config: build_config(opts), endpoints: %{}, waiters: %{}}}
  end

  @impl GenServer
  def handle_call({:acquire, url, class, weight, key}, {pid, _tag} = from, state) do
    endpoint = endpoint(state, url)

    entry = %{
      from: from,
      ref: Process.monitor(pid),
      class: class,
      weight: min(weight, state.config.rps),
      key: key,
      queued_at: monotonic_ms()
    }

    state =
      state
      |> put_endpoint(url, %{endpoint | queue: endpoint.queue ++ [entry]})
      |> put_waiter(entry.ref, {:queued, url})
      |> pump(url)

    {:noreply, state}
  end

  def handle_call({:stats, url}, _from, state) do
    endpoint = endpoint(state, url)

    stats = %{
      inflight: endpoint.inflight,
      inflight_total: endpoint.inflight_total,
      queue_depth: length(endpoint.queue)
    }

    {:reply, stats, state}
  end

  @impl GenServer
  def handle_cast({:complete, ticket, result}, state) do
    {:noreply, state |> finish(ticket, {:coalesced, result}) |> pump(ticket.url)}
  end

  def handle_cast({:release, ticket}, state) do
    {:noreply, state |> finish(ticket, :release) |> pump(ticket.url)}
  end

  def handle_cast({:abandon, ticket}, state) do
    {:noreply, state |> finish(ticket, :promote) |> pump(ticket.url)}
  end

  @impl GenServer
  def handle_info({:pump, url}, state) do
    endpoint = %{endpoint(state, url) | timer: nil}

    {:noreply, state |> put_endpoint(url, endpoint) |> pump(url)}
  end

  def handle_info({:DOWN, ref, :process, _pid, _reason}, state) do
    case Map.fetch(state.waiters, ref) do
      {:ok, {:queued, url}} ->
        endpoint = endpoint(state, url)
        queue = Enum.reject(endpoint.queue, &(&1.ref == ref))

        {:noreply,
         state |> drop_waiter(ref) |> put_endpoint(url, %{endpoint | queue: queue}) |> pump(url)}

      {:ok, {:running, url, class, weight}} ->
        ticket = %{url: url, class: class, weight: weight, key: nil, ref: ref}

        {:noreply, state |> finish(ticket, :promote) |> pump(url)}

      {:ok, {:leader, url, key, class, weight}} ->
        ticket = %{url: url, class: class, weight: weight, key: key, ref: ref}

        {:noreply, state |> finish(ticket, :promote) |> pump(url)}

      {:ok, {:follower, url, key}} ->
        endpoint = endpoint(state, url)

        leaders =
          case Map.fetch(endpoint.leaders, key) do
            {:ok, leader} ->
              Map.put(endpoint.leaders, key, %{
                leader
                | followers:
                    Enum.reject(leader.followers, fn {_from, follower_ref} ->
                      follower_ref == ref
                    end)
              })

            :error ->
              endpoint.leaders
          end

        {:noreply, state |> drop_waiter(ref) |> put_endpoint(url, %{endpoint | leaders: leaders})}

      :error ->
        {:noreply, state}
    end
  end

  defp acquire(url, class, weight, key) do
    GenServer.call(__MODULE__, {:acquire, url, class, weight, key}, :infinity)
  catch
    :exit, _reason ->
      Logger.debug(fn -> "JSON RPC node guard is unavailable, waiting for it to restart" end)

      Process.sleep(@unavailable_retry_interval)

      acquire(url, class, weight, key)
  end

  defp run(ticket, fun) do
    result = fun.()

    # Only a request that can be coalesced has to hand its response to the guard, so a batch
    # response, which is the largest payload the transport returns, is never copied into the
    # guard's mailbox.
    if is_nil(ticket.key) do
      GenServer.cast(__MODULE__, {:release, ticket})
    else
      GenServer.cast(__MODULE__, {:complete, ticket, result})
    end

    result
  catch
    kind, reason ->
      GenServer.cast(__MODULE__, {:abandon, ticket})

      :erlang.raise(kind, reason, __STACKTRACE__)
  end

  defp build_config(opts) do
    merged = Keyword.merge(Application.get_env(:ethereum_jsonrpc, __MODULE__, []), opts)

    %{
      max_inflight: positive(merged[:max_inflight], @default_max_inflight),
      rps: positive(merged[:rps], @default_rps),
      trace_max_inflight: positive(merged[:trace_max_inflight], @default_trace_max_inflight),
      archive_max_inflight:
        positive(merged[:archive_max_inflight], @default_archive_max_inflight),
      coalesce?:
        if(is_boolean(merged[:coalesce?]), do: merged[:coalesce?], else: @default_coalesce?)
    }
  end

  defp positive(value, _default) when is_integer(value) and value > 0, do: value
  defp positive(_value, default), do: default

  defp pump(state, url) do
    config = state.config
    endpoint = state |> endpoint(url) |> refill(config)

    {state, endpoint, kept} =
      Enum.reduce(endpoint.queue, {state, endpoint, []}, fn entry,
                                                            {acc_state, acc_endpoint, kept} ->
        cond do
          coalescable_entry?(acc_endpoint, entry) ->
            {acc_state, acc_endpoint} = attach_follower(acc_state, acc_endpoint, url, entry)
            {acc_state, acc_endpoint, kept}

          admissible?(acc_endpoint, config, entry) ->
            {acc_state, acc_endpoint} = admit(acc_state, acc_endpoint, config, url, entry)
            {acc_state, acc_endpoint, kept}

          true ->
            {acc_state, acc_endpoint, [entry | kept]}
        end
      end)

    endpoint =
      %{endpoint | queue: Enum.reverse(kept)}
      |> schedule_pump(url, config)

    publish(endpoint)

    put_endpoint(state, url, endpoint)
  end

  defp admissible?(endpoint, config, entry) do
    endpoint.inflight_total < config.max_inflight and
      Map.fetch!(endpoint.inflight, entry.class) < class_limit(config, entry.class) and
      endpoint.tokens >= entry.weight
  end

  defp class_limit(config, :trace), do: config.trace_max_inflight
  defp class_limit(config, :archive), do: config.archive_max_inflight
  defp class_limit(config, :default), do: config.max_inflight

  defp coalescable_entry?(endpoint, entry) do
    not is_nil(entry.key) and Map.has_key?(endpoint.leaders, entry.key)
  end

  defp attach_follower(state, endpoint, url, entry) do
    Metrics.coalesced(entry.class)

    leaders =
      Map.update!(endpoint.leaders, entry.key, fn leader ->
        %{leader | followers: leader.followers ++ [{entry.from, entry.ref}]}
      end)

    {put_waiter(state, entry.ref, {:follower, url, entry.key}), %{endpoint | leaders: leaders}}
  end

  defp admit(state, endpoint, config, url, entry) do
    Metrics.wait_time(entry.class, monotonic_ms() - entry.queued_at)

    ticket = %{url: url, class: entry.class, weight: entry.weight, key: entry.key, ref: entry.ref}

    GenServer.reply(entry.from, {:run, ticket})

    endpoint = reserve(endpoint, entry)

    if config.coalesce? and not is_nil(entry.key) do
      leaders = Map.put(endpoint.leaders, entry.key, %{ref: entry.ref, followers: []})

      {put_waiter(state, entry.ref, {:leader, url, entry.key, entry.class, entry.weight}),
       %{endpoint | leaders: leaders}}
    else
      {put_waiter(state, entry.ref, {:running, url, entry.class, entry.weight}), endpoint}
    end
  end

  defp reserve(endpoint, entry) do
    %{
      endpoint
      | inflight_total: endpoint.inflight_total + 1,
        inflight: Map.update!(endpoint.inflight, entry.class, &(&1 + 1)),
        tokens: endpoint.tokens - entry.weight
    }
  end

  defp release(endpoint, class) do
    %{
      endpoint
      | inflight_total: max(endpoint.inflight_total - 1, 0),
        inflight: Map.update!(endpoint.inflight, class, &max(&1 - 1, 0))
    }
  end

  defp finish(state, %{url: url, class: class, key: key, ref: ref} = ticket, disposition) do
    Process.demonitor(ref, [:flush])

    state = drop_waiter(state, ref)
    endpoint = endpoint(state, url)

    case key && Map.get(endpoint.leaders, key) do
      nil ->
        put_endpoint(state, url, release(endpoint, class))

      %{ref: leader_ref} when leader_ref != ref ->
        put_endpoint(state, url, release(endpoint, class))

      %{followers: followers} ->
        resolve_leader(state, url, endpoint, ticket, followers, disposition)
    end
  end

  defp resolve_leader(
         state,
         url,
         endpoint,
         %{class: class, key: key},
         followers,
         {:coalesced, _} = reply
       ) do
    state =
      Enum.reduce(followers, state, fn {from, follower_ref}, acc ->
        Process.demonitor(follower_ref, [:flush])
        GenServer.reply(from, reply)
        drop_waiter(acc, follower_ref)
      end)

    endpoint = %{endpoint | leaders: Map.delete(endpoint.leaders, key)}

    put_endpoint(state, url, release(endpoint, class))
  end

  defp resolve_leader(state, url, endpoint, %{class: class, key: key}, [], :promote) do
    endpoint = %{endpoint | leaders: Map.delete(endpoint.leaders, key)}

    put_endpoint(state, url, release(endpoint, class))
  end

  defp resolve_leader(state, url, endpoint, ticket, [{from, follower_ref} | rest], :promote) do
    %{class: class, weight: weight, key: key} = ticket

    GenServer.reply(
      from,
      {:run, %{url: url, class: class, weight: weight, key: key, ref: follower_ref}}
    )

    endpoint = %{
      endpoint
      | leaders: Map.put(endpoint.leaders, key, %{ref: follower_ref, followers: rest})
    }

    state
    |> put_waiter(follower_ref, {:leader, url, key, class, weight})
    |> put_endpoint(url, endpoint)
  end

  defp refill(endpoint, config) do
    now = monotonic_ms()
    elapsed = now - endpoint.refilled_at

    if elapsed > 0 do
      tokens = min(config.rps * 1.0, endpoint.tokens + elapsed * config.rps / 1000)

      %{endpoint | tokens: tokens, refilled_at: now}
    else
      endpoint
    end
  end

  defp schedule_pump(%{queue: []} = endpoint, _url, _config), do: endpoint

  defp schedule_pump(%{timer: timer} = endpoint, _url, _config) when is_reference(timer),
    do: endpoint

  defp schedule_pump(endpoint, url, config) do
    cheapest = endpoint.queue |> Enum.map(& &1.weight) |> Enum.min()
    deficit = cheapest - endpoint.tokens

    delay =
      if deficit <= 0 do
        @max_pump_interval
      else
        deficit
        |> Kernel.*(1000)
        |> Kernel./(config.rps)
        |> ceil()
        |> min(@max_pump_interval)
        |> max(1)
      end

    %{endpoint | timer: Process.send_after(self(), {:pump, url}, delay)}
  end

  defp publish(endpoint) do
    queued = Enum.frequencies_by(endpoint.queue, & &1.class)

    Enum.each(@classes, fn class ->
      Metrics.queue_depth(class, Map.get(queued, class, 0))
      Metrics.inflight(class, Map.fetch!(endpoint.inflight, class))
    end)
  end

  defp endpoint(state, url) do
    Map.get_lazy(state.endpoints, url, fn -> new_endpoint(state.config) end)
  end

  defp new_endpoint(config) do
    %{
      inflight: Map.new(@classes, &{&1, 0}),
      inflight_total: 0,
      tokens: config.rps * 1.0,
      refilled_at: monotonic_ms(),
      queue: [],
      leaders: %{},
      timer: nil
    }
  end

  defp put_endpoint(state, url, endpoint) do
    %{state | endpoints: Map.put(state.endpoints, url, endpoint)}
  end

  defp put_waiter(state, ref, waiter) do
    %{state | waiters: Map.put(state.waiters, ref, waiter)}
  end

  defp drop_waiter(state, ref) do
    %{state | waiters: Map.delete(state.waiters, ref)}
  end

  defp classify_method(request) when is_map(request) do
    method = field(request, :method)
    params = field(request, :params) || []

    cond do
      not is_binary(method) -> :default
      String.starts_with?(method, @trace_method_prefixes) -> :trace
      historical_state_read?(method, params) -> :archive
      true -> :default
    end
  end

  defp classify_method(_request), do: :default

  defp historical_state_read?(method, params) when is_list(params) do
    case Map.fetch(@historical_block_parameter_index, method) do
      {:ok, index} -> params |> Enum.at(index) |> historical_block?()
      :error -> false
    end
  end

  defp historical_state_read?(_method, _params), do: false

  defp historical_block?(nil), do: false
  defp historical_block?(block) when block in @latest_block_parameters, do: false
  defp historical_block?(block) when is_binary(block), do: true
  defp historical_block?(_block), do: false

  defp heavier(:trace, _other), do: :trace
  defp heavier(_class, :trace), do: :trace
  defp heavier(:archive, _other), do: :archive
  defp heavier(_class, :archive), do: :archive
  defp heavier(_class, _other), do: :default

  defp coalesce_key(_url, batch) when is_list(batch), do: nil

  defp coalesce_key(url, request) when is_map(request) do
    method = field(request, :method)

    if is_binary(method) and coalescable?(method) do
      {url, method, field(request, :params) || []}
    end
  end

  defp coalescable?(method) do
    method in @coalescable_methods or String.starts_with?(method, @trace_method_prefixes)
  end

  defp field(request, key) do
    case Map.fetch(request, key) do
      {:ok, value} -> value
      :error -> Map.get(request, Atom.to_string(key))
    end
  end

  defp monotonic_ms, do: System.monotonic_time(:millisecond)
end
