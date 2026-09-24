defmodule EthereumJSONRPC.HTTP.Throttle.Metrics do
  @moduledoc """
  Prometheus metrics for `EthereumJSONRPC.HTTP.Throttle`.

  All metrics are labelled by the request class (`default`, `archive` or `trace`) only. The
  endpoint URL is deliberately never used as a label because it can carry credentials.
  """

  use Prometheus.Metric

  require Logger

  @wait_buckets [1, 5, 10, 25, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000]

  @gauge [
    name: :json_rpc_throttle_queue_depth,
    labels: [:class],
    help: "Number of JSON RPC requests waiting for a slot on the node guard"
  ]

  @gauge [
    name: :json_rpc_throttle_inflight,
    labels: [:class],
    help: "Number of JSON RPC requests currently in flight against the node"
  ]

  @histogram [
    name: :json_rpc_throttle_wait_time_milliseconds,
    labels: [:class],
    buckets: @wait_buckets,
    duration_unit: false,
    help: "Time a JSON RPC request spent waiting for a slot on the node guard"
  ]

  @counter [
    name: :json_rpc_throttle_coalesced_count,
    labels: [:class],
    help: "Number of JSON RPC requests served by an already in-flight identical request"
  ]

  @doc """
  Publishes the queue depth of `class`.
  """
  @spec queue_depth(atom(), non_neg_integer()) :: :ok
  def queue_depth(class, depth) do
    Gauge.set([name: :json_rpc_throttle_queue_depth, labels: [class]], depth)

    if depth > 0 do
      Logger.debug(fn -> "JSON RPC node guard queue depth for #{class}: #{depth}" end)
    end

    :ok
  end

  @doc """
  Publishes the number of in-flight requests of `class`.
  """
  @spec inflight(atom(), non_neg_integer()) :: :ok
  def inflight(class, count) do
    Gauge.set([name: :json_rpc_throttle_inflight, labels: [class]], count)
  end

  @doc """
  Publishes how long a request of `class` waited before it was admitted.
  """
  @spec wait_time(atom(), non_neg_integer()) :: :ok
  def wait_time(class, milliseconds) do
    Histogram.observe(
      [name: :json_rpc_throttle_wait_time_milliseconds, labels: [class]],
      milliseconds
    )

    if milliseconds > 0 do
      Logger.debug(fn ->
        "JSON RPC node guard admitted a #{class} request after #{milliseconds} ms"
      end)
    end

    :ok
  end

  @doc """
  Counts a request of `class` that was served by an already in-flight identical request.
  """
  @spec coalesced(atom()) :: :ok
  def coalesced(class) do
    Counter.inc(name: :json_rpc_throttle_coalesced_count, labels: [class])
  end
end
