defmodule Indexer.Memory.MonitorTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog

  alias Indexer.BufferedTask
  alias Indexer.Memory.Monitor
  alias Indexer.Memory.Shrinkable

  @initial_entry_count 40
  @max_batch_size 2
  @assert_receive_timeout 5_000

  @moduletag :capture_log

  defmodule QueuedTask do
    @moduledoc false

    @behaviour BufferedTask

    @initial_entry_count 40
    @release_timeout 30_000

    @impl BufferedTask
    def init(initial, reducer, _state) do
      Enum.reduce(1..@initial_entry_count, initial, fn entry, acc -> reducer.(entry, acc) end)
    end

    # Holds the single worker so that the rest of the initial entries stay in the queue where the
    # memory monitor can shrink them.
    @impl BufferedTask
    def run(batch, test_pid) do
      send(test_pid, {:running, self(), batch})

      receive do
        :release -> :ok
      after
        @release_timeout -> :ok
      end
    end
  end

  setup do
    start_supervised!({Task.Supervisor, name: MonitorTestTaskSupervisor})

    monitor =
      start_supervised!(
        {Monitor, [%{limit: 1, timer_interval: :timer.hours(1)}, [name: MonitorTestMemoryMonitor]]},
        id: MonitorTestMemoryMonitor
      )

    buffered_task =
      start_supervised!(
        {BufferedTask,
         [
           {QueuedTask,
            state: self(),
            task_supervisor: MonitorTestTaskSupervisor,
            flush_interval: :timer.hours(1),
            max_batch_size: @max_batch_size,
            max_concurrency: 1,
            poll: false,
            memory_monitor: MonitorTestMemoryMonitor},
           [name: QueuedTask]
         ]},
        id: QueuedTask
      )

    assert_receive {:running, worker, _batch}, @assert_receive_timeout

    on_exit(fn -> send(worker, :release) end)

    # every entry except the batch the blocked worker holds ends up in the queue
    queued_entry_count = @initial_entry_count - @max_batch_size

    wait_for_queued_entries(buffered_task, queued_entry_count)

    %{monitor: monitor, buffered_task: buffered_task, queued_entry_count: queued_entry_count}
  end

  describe "handle_info(:check, state)" do
    test "logs the fetcher and the count of entries shed when the memory limit is surpassed", %{
      monitor: monitor,
      buffered_task: buffered_task,
      queued_entry_count: entry_count_before
    } do
      log =
        capture_log(fn ->
          send(monitor, :check)
          # answered only once `:check` has been handled
          :sys.get_state(monitor)
          Logger.flush()
        end)

      entry_count_after = Shrinkable.queued_entry_count(buffered_task)

      assert entry_count_after < entry_count_before

      assert log =~
               "Memory limit surpassed: MonitorTest.QueuedTask shed #{entry_count_before - entry_count_after} queued entries while shrinking"

      assert log =~ "(#{entry_count_before} entries before, #{entry_count_after} entries after)"
    end
  end

  defp wait_for_queued_entries(pid, expected_count, attempts_left \\ 100) do
    case Shrinkable.queued_entry_count(pid) do
      ^expected_count ->
        :ok

      _count when attempts_left > 0 ->
        Process.sleep(20)
        wait_for_queued_entries(pid, expected_count, attempts_left - 1)

      count ->
        flunk("expected #{expected_count} queued entries, got #{count}")
    end
  end
end
