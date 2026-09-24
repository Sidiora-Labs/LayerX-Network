defmodule EthereumJSONRPC.HTTP.ThrottleTest.CountingTransport do
  @moduledoc """
  `EthereumJSONRPC.HTTP` implementation used by `EthereumJSONRPC.HTTP.ThrottleTest` that records how
  many requests of each kind are in flight at the same time.

  It classifies the payload on its own, independently of `EthereumJSONRPC.HTTP.Throttle`, so the
  concurrency assertions do not depend on the code under test.
  """

  @behaviour EthereumJSONRPC.HTTP

  @latest_block_parameters ~w(latest pending)

  def start_link do
    Agent.start_link(fn -> %{current: %{}, max: %{}, calls: 0} end, name: __MODULE__)
  end

  @doc """
  Highest number of `kind` requests that were in flight at the same time.
  """
  @spec max_concurrency(atom()) :: non_neg_integer()
  def max_concurrency(kind), do: Agent.get(__MODULE__, &Map.get(&1.max, kind, 0))

  @doc """
  Total number of requests that reached the transport.
  """
  @spec calls() :: non_neg_integer()
  def calls, do: Agent.get(__MODULE__, & &1.calls)

  @impl EthereumJSONRPC.HTTP
  def json_rpc(_url, json, _headers, options) do
    decoded = json |> IO.iodata_to_binary() |> Jason.decode!()
    kind = kind(decoded)

    enter(kind)

    options |> Keyword.get(:test_delay, 25) |> Process.sleep()

    leave(kind)

    {:ok, %{body: Jason.encode!(response(decoded)), status_code: 200}}
  end

  defp kind(payload) when is_list(payload), do: :batch

  defp kind(%{"method" => "debug_traceTransaction"}), do: :trace

  defp kind(%{"method" => "eth_getBalance", "params" => [_address, block]})
       when block not in @latest_block_parameters,
       do: :archive

  defp kind(_payload), do: :default

  defp response(payload) when is_list(payload), do: Enum.map(payload, &response/1)

  defp response(%{"id" => id}), do: %{"jsonrpc" => "2.0", "id" => id, "result" => "0x1"}

  defp enter(kind) do
    Agent.update(__MODULE__, fn state ->
      current = Map.update(state.current, kind, 1, &(&1 + 1))
      in_flight = Map.fetch!(current, kind)
      maxima = Map.update(state.max, kind, in_flight, &max(&1, in_flight))

      %{state | current: current, max: maxima, calls: state.calls + 1}
    end)
  end

  defp leave(kind) do
    Agent.update(__MODULE__, fn state ->
      %{state | current: Map.update!(state.current, kind, &(&1 - 1))}
    end)
  end
end

defmodule EthereumJSONRPC.HTTP.ThrottleTest do
  use ExUnit.Case, async: false

  alias EthereumJSONRPC.HTTP
  alias EthereumJSONRPC.HTTP.Throttle
  alias EthereumJSONRPC.HTTP.ThrottleTest.CountingTransport

  @url "https://example.com"

  @unlimited_rps 100_000

  setup do
    {:ok, _agent} = CountingTransport.start_link()

    original = Application.get_env(:ethereum_jsonrpc, Throttle)

    on_exit(fn ->
      if is_nil(original) do
        Application.delete_env(:ethereum_jsonrpc, Throttle)
      else
        Application.put_env(:ethereum_jsonrpc, Throttle, original)
      end

      restart_guard()
    end)

    :ok
  end

  describe "request/3 concurrency caps" do
    test "never lets more requests than the global cap reach the node and queues the rest" do
      configure(
        max_inflight: 4,
        rps: @unlimited_rps,
        trace_max_inflight: 4,
        archive_max_inflight: 4
      )

      results = run_concurrently(1..40, &block_request/1)

      assert Enum.all?(results, &match?({:ok, "0x1"}, &1))
      assert CountingTransport.calls() == 40
      assert CountingTransport.max_concurrency(:default) <= 4
      assert CountingTransport.max_concurrency(:default) >= 2
    end

    test "holds tracing calls to their own lower cap" do
      configure(
        max_inflight: 16,
        rps: @unlimited_rps,
        trace_max_inflight: 2,
        archive_max_inflight: 16
      )

      results = run_concurrently(1..20, &trace_request/1)

      assert Enum.all?(results, &match?({:ok, "0x1"}, &1))
      assert CountingTransport.calls() == 20
      assert CountingTransport.max_concurrency(:trace) <= 2
    end

    test "holds historical balance reads to their own lower cap" do
      configure(
        max_inflight: 16,
        rps: @unlimited_rps,
        trace_max_inflight: 16,
        archive_max_inflight: 2
      )

      results = run_concurrently(1..20, &historical_balance_request/1)

      assert Enum.all?(results, &match?({:ok, "0x1"}, &1))
      assert CountingTransport.calls() == 20
      assert CountingTransport.max_concurrency(:archive) <= 2
    end

    test "leaves balance reads at the latest block outside the historical cap" do
      configure(
        max_inflight: 16,
        rps: @unlimited_rps,
        trace_max_inflight: 16,
        archive_max_inflight: 2
      )

      results = run_concurrently(1..20, &latest_balance_request/1)

      assert Enum.all?(results, &match?({:ok, "0x1"}, &1))
      assert CountingTransport.max_concurrency(:archive) == 0
      assert CountingTransport.max_concurrency(:default) > 2
    end

    test "counts a batch as a single request against the concurrency cap" do
      configure(
        max_inflight: 2,
        rps: @unlimited_rps,
        trace_max_inflight: 2,
        archive_max_inflight: 2
      )

      results =
        run_concurrently(1..8, fn id ->
          HTTP.json_rpc(
            Enum.map(1..5, &block_request_payload(id * 100 + &1)),
            transport_options()
          )
        end)

      assert Enum.all?(results, &match?({:ok, _}, &1))
      assert CountingTransport.calls() == 8
      assert CountingTransport.max_concurrency(:batch) <= 2
    end
  end

  describe "request/3 rate budget" do
    test "spreads requests over the configured per-second budget" do
      configure(max_inflight: 64, rps: 10, trace_max_inflight: 64, archive_max_inflight: 64)

      {microseconds, results} =
        :timer.tc(fn -> run_concurrently(1..20, &block_request(&1, test_delay: 0)) end)

      assert Enum.all?(results, &match?({:ok, "0x1"}, &1))
      assert CountingTransport.calls() == 20

      elapsed = div(microseconds, 1000)
      assert elapsed >= 800, "20 requests at 10 rps finished in #{elapsed} ms"
    end

    test "charges one budget token per sub-request of a batch" do
      configure(max_inflight: 64, rps: 4, trace_max_inflight: 64, archive_max_inflight: 64)

      {microseconds, results} =
        :timer.tc(fn ->
          run_concurrently(1..2, fn id ->
            HTTP.json_rpc(
              Enum.map(1..4, &block_request_payload(id * 100 + &1)),
              transport_options(test_delay: 0)
            )
          end)
        end)

      assert Enum.all?(results, &match?({:ok, _}, &1))

      elapsed = div(microseconds, 1000)
      assert elapsed >= 800, "two batches of four at 4 rps finished in #{elapsed} ms"
    end
  end

  describe "request/3 coalescing" do
    test "serves identical concurrent requests from a single call to the node" do
      configure(
        max_inflight: 8,
        rps: @unlimited_rps,
        trace_max_inflight: 8,
        archive_max_inflight: 8
      )

      results = run_concurrently(1..6, fn _id -> block_request(1, test_delay: 100) end)

      assert results == List.duplicate({:ok, "0x1"}, 6)
      assert CountingTransport.calls() == 1
    end

    test "sends every request separately when coalescing is disabled" do
      configure(
        max_inflight: 8,
        rps: @unlimited_rps,
        trace_max_inflight: 8,
        archive_max_inflight: 8,
        coalesce?: false
      )

      results = run_concurrently(1..6, fn _id -> block_request(1, test_delay: 100) end)

      assert results == List.duplicate({:ok, "0x1"}, 6)
      assert CountingTransport.calls() == 6
    end
  end

  describe "request/3 availability" do
    test "reports queue depth and in-flight counts while callers wait" do
      configure(
        max_inflight: 1,
        rps: @unlimited_rps,
        trace_max_inflight: 1,
        archive_max_inflight: 1,
        coalesce?: false
      )

      tasks =
        Enum.map(1..4, fn id -> Task.async(fn -> block_request(id, test_delay: 500) end) end)

      wait_until(fn ->
        stats = Throttle.stats(@url)

        stats.inflight_total == 1 and stats.queue_depth == 3
      end)

      assert Enum.all?(Task.await_many(tasks, 30_000), &match?({:ok, "0x1"}, &1))
      assert Throttle.stats(@url).queue_depth == 0
    end

    test "waiting callers are served by the restarted guard instead of bypassing it" do
      configure(
        max_inflight: 1,
        rps: @unlimited_rps,
        trace_max_inflight: 1,
        archive_max_inflight: 1,
        coalesce?: false
      )

      holder = Task.async(fn -> block_request(1, test_delay: 500) end)
      wait_until(fn -> Throttle.stats(@url).inflight_total == 1 end)

      queued = Task.async(fn -> block_request(2, test_delay: 50) end)
      wait_until(fn -> Throttle.stats(@url).queue_depth == 1 end)

      guard = Process.whereis(Throttle)
      reference = Process.monitor(guard)
      Process.exit(guard, :kill)
      assert_receive {:DOWN, ^reference, :process, ^guard, :killed}, 5_000

      assert {:ok, "0x1"} = Task.await(holder, 30_000)
      assert {:ok, "0x1"} = Task.await(queued, 30_000)
      assert CountingTransport.calls() == 2

      wait_until(fn -> is_pid(Process.whereis(Throttle)) end)
      assert {:ok, "0x1"} = block_request(3)
    end
  end

  describe "classify/1" do
    test "separates tracing calls, historical state reads and everything else" do
      assert {:trace, 1} =
               Throttle.classify(block_request_payload(1, "debug_traceTransaction", ["0x1", %{}]))

      assert {:trace, 1} = Throttle.classify(block_request_payload(1, "trace_block", ["0x1"]))

      assert {:archive, 1} =
               Throttle.classify(block_request_payload(1, "eth_getBalance", ["0xabc", "0x1"]))

      assert {:archive, 1} =
               Throttle.classify(
                 block_request_payload(1, "eth_getStorageAt", ["0xabc", "0x0", "0x1"])
               )

      assert {:default, 1} =
               Throttle.classify(block_request_payload(1, "eth_getBalance", ["0xabc", "latest"]))

      assert {:default, 1} =
               Throttle.classify(block_request_payload(1, "eth_getBalance", ["0xabc", "pending"]))

      assert {:default, 1} = Throttle.classify(block_request_payload(1, "eth_blockNumber", []))
    end

    test "a batch takes the heaviest class of its members and one token per member" do
      batch = [
        block_request_payload(1, "eth_blockNumber", []),
        block_request_payload(2, "eth_getBalance", ["0xabc", "0x1"]),
        block_request_payload(3, "debug_traceTransaction", ["0x1", %{}])
      ]

      assert {:trace, 3} = Throttle.classify(batch)
      assert {:archive, 2} = Throttle.classify(Enum.take(batch, 2))
      assert {:default, 1} = Throttle.classify(Enum.take(batch, 1))
    end
  end

  defp configure(opts) do
    Application.put_env(:ethereum_jsonrpc, Throttle, opts)

    restart_guard()
  end

  defp restart_guard do
    :ok = Supervisor.terminate_child(EthereumJSONRPC.Supervisor, Throttle)
    {:ok, _pid} = Supervisor.restart_child(EthereumJSONRPC.Supervisor, Throttle)

    :ok
  end

  defp run_concurrently(enumerable, fun) do
    enumerable
    |> Task.async_stream(fun, max_concurrency: Enum.count(enumerable), timeout: 60_000)
    |> Enum.map(fn {:ok, result} -> result end)
  end

  defp block_request(id, http_options \\ []) do
    HTTP.json_rpc(block_request_payload(id), transport_options(http_options))
  end

  defp trace_request(id, http_options \\ []) do
    id
    |> block_request_payload("debug_traceTransaction", [
      EthereumJSONRPC.integer_to_quantity(id),
      %{}
    ])
    |> HTTP.json_rpc(transport_options(http_options))
  end

  defp historical_balance_request(id, http_options \\ []) do
    id
    |> block_request_payload("eth_getBalance", [
      address(id),
      EthereumJSONRPC.integer_to_quantity(id)
    ])
    |> HTTP.json_rpc(transport_options(http_options))
  end

  defp latest_balance_request(id, http_options \\ []) do
    id
    |> block_request_payload("eth_getBalance", [address(id), "latest"])
    |> HTTP.json_rpc(transport_options(http_options))
  end

  defp block_request_payload(id, method \\ "eth_getBlockByNumber", params \\ nil) do
    EthereumJSONRPC.request(%{
      id: id,
      method: method,
      params: params || [EthereumJSONRPC.integer_to_quantity(id), true]
    })
  end

  defp address(id), do: "0x" <> String.pad_leading(Integer.to_string(id, 16), 40, "0")

  defp transport_options(http_options \\ []) do
    [http: CountingTransport, urls: [@url], http_options: http_options]
  end

  defp wait_until(fun, remaining \\ 200) do
    cond do
      fun.() ->
        :ok

      remaining == 0 ->
        flunk("condition was not met in time")

      true ->
        Process.sleep(25)
        wait_until(fun, remaining - 1)
    end
  end
end
