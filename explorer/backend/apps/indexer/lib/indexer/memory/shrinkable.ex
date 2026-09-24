defmodule Indexer.Memory.Shrinkable do
  @moduledoc """
  A process that can shrink its memory usage when asked by `Indexer.Memory.Monitor`.

  Processes need to `handle_call(:shrink, from, state)`, `handle_call(:shrunk?, from, state)`,
  `handle_call(:expand, from, state)` and `handle_call(:queue_size, from, state)`.
  `Indexer.BufferedTask` implements all of them.
  """

  @doc """
  Asks `pid` to shrink its memory usage.
  """
  @spec shrink(pid()) :: :ok | {:error, :minimum_size}
  def shrink(pid) when is_pid(pid) do
    GenServer.call(pid, :shrink)
  end

  @doc """
  Asks `pid` if it was shrunk in the past.

  `pid` will only return `true` if it returned `:ok` from `shrink/1`.
  """
  @spec shrunk?(pid()) :: boolean()
  def shrunk?(pid) when is_pid(pid) do
    GenServer.call(pid, :shrunk?)
  end

  @doc """
  Asks `pid` to expand its size
  """
  @spec expand(pid()) :: :ok
  def expand(pid) when is_pid(pid) do
    GenServer.call(pid, :expand)
  end

  @doc """
  Asks `pid` how many entries its shrinkable queue currently holds.

  Used by `Indexer.Memory.Monitor` to report how much queued work a shrink shed.  Only the queue is
  counted, because `shrink/1` drops entries from the queue and from nowhere else.
  """
  @spec queued_entry_count(pid()) :: non_neg_integer()
  def queued_entry_count(pid) when is_pid(pid) do
    GenServer.call(pid, :queue_size)
  end
end
