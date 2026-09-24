defmodule Indexer.Block.Realtime.BatchTest do
  use EthereumJSONRPC.Case, async: false
  use Explorer.DataCase

  import EthereumJSONRPC, only: [integer_to_quantity: 1, quantity_to_integer: 1]
  import Mox

  alias Explorer.Chain.Block
  alias Explorer.Utility.MissingBlockRange
  alias Indexer.Block.Realtime
  alias Indexer.Fetcher.CoinBalance.Realtime, as: CoinBalanceRealtime
  alias Indexer.Fetcher.OnDemand.ContractCreator, as: ContractCreatorOnDemand

  alias Indexer.Fetcher.{
    ContractCode,
    InternalTransaction,
    ReplacedTransaction,
    Token,
    UncleBlock
  }

  @moduletag capture_log: true

  @head 3_946_100
  @wait_timeout 15_000

  # MUST use global mode because the import runs inside tasks of `Indexer.Block.Realtime.TaskSupervisor`, whose pids are
  # not known to the test process.
  setup :set_mox_global

  setup %{json_rpc_named_arguments: json_rpc_named_arguments} do
    initial_indexer_env = Application.get_all_env(:indexer)

    Application.put_env(:indexer, :fetch_rewards_way, "manual")
    Application.put_env(:indexer, InternalTransaction.Supervisor, disabled?: true)
    Application.put_env(:indexer, UncleBlock.Supervisor, disabled?: true)

    Application.put_env(:indexer, Indexer.Fetcher.Celo.EpochBlockOperations.Supervisor, disabled?: true)

    Application.put_env(:indexer, Indexer.Fetcher.TokenInstance.Realtime.Supervisor, disabled?: true)

    # the realtime coin balance fetcher is started per test, because its daily balance step fetches one block per
    # buffered entry and would otherwise hide which JSON RPC calls the range import itself made
    Application.put_env(:indexer, CoinBalanceRealtime.Supervisor, disabled?: true)

    on_exit(fn ->
      Application.delete_env(:indexer, InternalTransaction.Supervisor)
      Application.delete_env(:indexer, UncleBlock.Supervisor)
      Application.delete_env(:indexer, CoinBalanceRealtime.Supervisor)
      Application.put_all_env([{:indexer, initial_indexer_env}])
    end)

    start_supervised!({Task.Supervisor, name: Indexer.TaskSupervisor})
    start_supervised!({Task.Supervisor, name: Realtime.TaskSupervisor})

    block_fetcher = %Indexer.Block.Fetcher{
      broadcast: false,
      callback_module: Realtime.Fetcher,
      json_rpc_named_arguments: json_rpc_named_arguments,
      task_supervisor: Indexer.TaskSupervisor
    }

    Token.Supervisor.Case.start_supervised!(json_rpc_named_arguments: json_rpc_named_arguments)

    ContractCode.Supervisor.Case.start_supervised!(json_rpc_named_arguments: json_rpc_named_arguments)

    ReplacedTransaction.Supervisor.Case.start_supervised!()
    {:ok, _pid} = ContractCreatorOnDemand.start_link([[], []])

    %{block_fetcher: block_fetcher, json_rpc_named_arguments: json_rpc_named_arguments}
  end

  describe "start_fetch_and_import/4" do
    @tag :no_geth
    test "imports the whole contiguous range up to the new head with one batched request per step",
         %{
           block_fetcher: block_fetcher,
           json_rpc_named_arguments: json_rpc_named_arguments
         } do
      start_coin_balance_fetcher(json_rpc_named_arguments)

      node = start_node(canonical_blocks((@head - 3)..@head))

      Realtime.Fetcher.start_fetch_and_import(@head, block_fetcher, @head - 4)

      assert_block_numbers((@head - 3)..@head)

      # the daily coin balance step fetches the block of every balance it writes, so only the first call is the one
      # the range import made
      assert [block_request | _] = requests_for(node, "eth_getBlockByNumber")

      assert Enum.map(block_request, &quantity_to_integer(hd(&1.params))) ==
               Enum.to_list((@head - 3)..@head)

      balance_requests = wait_until(fn -> single_call(requests_for(node, "eth_getBalance")) end)

      assert Enum.sort(Enum.map(balance_requests, & &1.params)) ==
               Enum.sort(
                 Enum.map((@head - 3)..@head, fn number ->
                   [miner_hash(number), integer_to_quantity(number)]
                 end)
               )

      assert Enum.all?(Repo.all(from(block in Block, select: block.consensus)))
    end

    @tag :no_geth
    test "fills the gap between the last imported block and the new head and clears its missing range",
         %{
           block_fetcher: block_fetcher
         } do
      node = start_node(canonical_blocks((@head - 5)..@head))

      MissingBlockRange.save_batch([(@head - 5)..@head//1])
      assert Repo.aggregate(MissingBlockRange, :count) > 0

      Realtime.Fetcher.start_fetch_and_import(@head, block_fetcher, @head - 6)

      assert_block_numbers((@head - 5)..@head)

      assert [block_request] = requests_for(node, "eth_getBlockByNumber")
      assert Enum.count(block_request) == 6

      assert wait_until(fn ->
               if Repo.aggregate(MissingBlockRange, :count) == 0, do: {:ok, :cleared}
             end) == :cleared
    end

    @tag :no_geth
    test "refetches the parent reported by a mismatching parent hash and flips consensus inside the range",
         %{
           block_fetcher: block_fetcher
         } do
      node = start_node(canonical_blocks((@head - 2)..@head))

      Realtime.Fetcher.start_fetch_and_import(@head, block_fetcher, @head - 3)

      assert_block_numbers((@head - 2)..@head)

      forked_head = @head + 1

      put_blocks(node, %{
        @head => block_data(@head, hash: forked_hash(@head), parent_hash: canonical_hash(@head - 1)),
        forked_head => block_data(forked_head, parent_hash: forked_hash(@head))
      })

      Realtime.Fetcher.start_fetch_and_import(forked_head, block_fetcher, @head, true)

      wait_until(fn ->
        if Repo.aggregate(Block, :count) == 5, do: {:ok, :reimported}
      end)

      consensus_by_hash =
        Repo.all(from(block in Block, select: {block.hash, block.number, block.consensus}))
        |> Map.new(fn {hash, number, consensus} -> {{to_string(hash), number}, consensus} end)

      assert consensus_by_hash[{canonical_hash(@head - 2), @head - 2}] == true
      assert consensus_by_hash[{canonical_hash(@head - 1), @head - 1}] == true
      assert consensus_by_hash[{canonical_hash(@head), @head}] == false
      assert consensus_by_hash[{forked_hash(@head), @head}] == true
      assert consensus_by_hash[{canonical_hash(forked_head), forked_head}] == true

      assert [_first_pass, reorg_request] = requests_for(node, "eth_getBlockByNumber")
      assert Enum.map(reorg_request, &quantity_to_integer(hd(&1.params))) == [@head, forked_head]
    end

    @tag :no_geth
    test "splits a range longer than the configured batch size into batch sized requests", %{
      block_fetcher: block_fetcher
    } do
      put_fetcher_env(batch_size: 2)

      node = start_node(canonical_blocks((@head - 4)..@head))

      Realtime.Fetcher.start_fetch_and_import(@head, block_fetcher, @head - 5)

      assert_block_numbers((@head - 4)..@head)

      requested_numbers =
        node
        |> requests_for("eth_getBlockByNumber")
        |> Enum.map(fn requests -> Enum.map(requests, &quantity_to_integer(hd(&1.params))) end)
        |> Enum.sort()

      assert requested_numbers == [
               [@head - 4, @head - 3],
               [@head - 2, @head - 1],
               [@head]
             ]
    end

    @tag :no_geth
    test "hands the part of the range above the maximum lag to the catchup fetcher", %{
      block_fetcher: block_fetcher
    } do
      put_fetcher_env(max_lag: 2)

      node = start_node(canonical_blocks((@head - 1)..@head))

      Realtime.Fetcher.start_fetch_and_import(@head, block_fetcher, @head - 6)

      assert_block_numbers((@head - 1)..@head)

      assert [block_request] = requests_for(node, "eth_getBlockByNumber")
      assert Enum.map(block_request, &quantity_to_integer(hd(&1.params))) == [@head - 1, @head]

      handed_off =
        MissingBlockRange
        |> Repo.all()
        |> Enum.map(&{&1.from_number, &1.to_number})

      assert handed_off == [{@head - 2, @head - 5}]
    end
  end

  # started with the production batch size instead of the shared support module's `max_batch_size: 1`, so that the
  # test sees the batching the realtime fetcher actually produces
  defp start_coin_balance_fetcher(json_rpc_named_arguments) do
    Application.put_env(:indexer, CoinBalanceRealtime.Supervisor, disabled?: false)

    start_supervised!(
      CoinBalanceRealtime.Supervisor.child_spec([
        [
          json_rpc_named_arguments: json_rpc_named_arguments,
          flush_interval: 50,
          max_batch_size: 500,
          max_concurrency: 1
        ]
      ])
    )
  end

  defp put_fetcher_env(overrides) do
    previous = Application.get_env(:indexer, Realtime.Fetcher) || []

    Application.put_env(:indexer, Realtime.Fetcher, Keyword.merge(previous, overrides))

    on_exit(fn -> Application.put_env(:indexer, Realtime.Fetcher, previous) end)
  end

  defp assert_block_numbers(expected_range) do
    expected = Enum.to_list(expected_range)

    numbers =
      wait_until(fn ->
        numbers =
          Repo.all(
            from(block in Block,
              where: block.consensus == true,
              select: block.number,
              order_by: block.number
            )
          )

        if numbers == expected, do: {:ok, numbers}
      end)

    assert numbers == expected
  end

  defp wait_until(fun) do
    wait_until(fun, System.monotonic_time(:millisecond) + @wait_timeout)
  end

  defp wait_until(fun, deadline) do
    case fun.() do
      {:ok, value} ->
        value

      _ ->
        if System.monotonic_time(:millisecond) >= deadline do
          flunk("condition was not met within #{@wait_timeout}ms")
        else
          Process.sleep(50)
          wait_until(fun, deadline)
        end
    end
  end

  defp single_call([requests]), do: {:ok, requests}
  defp single_call(_), do: nil

  # The node double answers every batched request it is given and records the batches it was given, so that a test can
  # assert on how many JSON RPC round trips a range import took.
  defp start_node(blocks_by_number) do
    {:ok, node} = Agent.start_link(fn -> %{blocks: blocks_by_number, calls: []} end)

    stub(EthereumJSONRPC.Mox, :json_rpc, fn requests, _options ->
      blocks =
        Agent.get_and_update(node, fn state ->
          {state.blocks, %{state | calls: state.calls ++ [requests]}}
        end)

      case requests do
        requests when is_list(requests) -> {:ok, Enum.map(requests, &response(&1, blocks))}
        request -> {:ok, response(request, blocks).result}
      end
    end)

    node
  end

  defp put_blocks(node, blocks_by_number) do
    Agent.update(node, fn state ->
      %{state | blocks: Map.merge(state.blocks, blocks_by_number)}
    end)
  end

  defp requests_for(node, method) do
    node
    |> Agent.get(& &1.calls)
    |> Enum.map(fn requests -> requests |> List.wrap() |> Enum.filter(&(&1.method == method)) end)
    |> Enum.reject(&Enum.empty?/1)
  end

  defp response(%{id: id, method: "eth_getBlockByNumber", params: [quantity, true]}, blocks) do
    %{id: id, jsonrpc: "2.0", result: Map.fetch!(blocks, quantity_to_integer(quantity))}
  end

  defp response(%{id: id, method: "eth_getBalance"}, _blocks) do
    %{id: id, jsonrpc: "2.0", result: "0x53474fa377a46000"}
  end

  defp response(%{id: id, method: method}, _blocks) do
    %{id: id, jsonrpc: "2.0", error: %{code: -32_601, message: "unexpected method " <> method}}
  end

  defp canonical_blocks(range) do
    Map.new(range, fn number -> {number, block_data(number)} end)
  end

  defp block_data(number, overrides \\ []) do
    hash = Keyword.get(overrides, :hash, canonical_hash(number))
    parent_hash = Keyword.get(overrides, :parent_hash, canonical_hash(number - 1))

    %{
      "author" => miner_hash(number),
      "difficulty" => "0xfffffffffffffffffffffffffffffffe",
      "extraData" => "0xd583010b088650617269747986312e32372e32826c69",
      "gasLimit" => "0x7a1200",
      "gasUsed" => "0x0",
      "hash" => hash,
      "logsBloom" => "0x" <> String.duplicate("0", 512),
      "miner" => miner_hash(number),
      "number" => integer_to_quantity(number),
      "parentHash" => parent_hash,
      "receiptsRoot" => "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
      "sealFields" => [
        "0x841246c640",
        "0xb84114db3fd7526b7ea3635f5c85c30dd8a645453aa2f8afe5fd33fe0ec663c9c7b653b0fb5d8dc7d0b809674fa9dca9887d1636a586bf62191da22255eb068bf20800"
      ],
      "sha3Uncles" => "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
      "signature" =>
        "14db3fd7526b7ea3635f5c85c30dd8a645453aa2f8afe5fd33fe0ec663c9c7b653b0fb5d8dc7d0b809674fa9dca9887d1636a586bf62191da22255eb068bf20800",
      "size" => "0x243",
      "stateRoot" => "0x3174c461989e9f99e08fa9b4ffb8bce8d9a281c8fc9f80694bb9d3acd4f15559",
      "step" => "306628160",
      "timestamp" => integer_to_quantity(1_533_000_000 + number),
      "totalDifficulty" => "0x3c365fffffffffffffffffffffffffed7f0360",
      "transactions" => [],
      "transactionsRoot" => "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
      "uncles" => []
    }
  end

  defp canonical_hash(number), do: hex(number * 2, 64)
  defp forked_hash(number), do: hex(number * 2 + 1, 64)
  defp miner_hash(number), do: hex(number, 40)

  defp hex(value, length) do
    "0x" <>
      (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(length, "0"))
  end
end
