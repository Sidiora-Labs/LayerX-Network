defmodule EthereumJSONRPC.PaxeerXTest do
  @moduledoc """
  Fixtures under `test/support/fixture/paxeer_x`:

    * `consecutive_blocks.json` and `new_head.json` are verbatim responses from a
      Paxeer X node - two consecutive `eth_getBlockByNumber` bodies and the
      `eth_subscribe("newHeads")` notification for the higher of the two.

    * `cosmos_originated_transaction.json` is the block-body entry and the shell
      receipt of a Cosmos-originated wasm execution, reconstructed field for
      field from the node's encoders, because the block range the node still
      serves contains no transactions at all. `logsBloom` is left at the empty
      bloom: the JSON-RPC layer under test discards it.
  """

  use EthereumJSONRPC.Case, async: false

  import Mox

  alias EthereumJSONRPC.{Blocks, PaxeerX, Receipt, Transaction}

  setup :verify_on_exit!

  @fixture_dir "test/support/fixture/paxeer_x"

  defp fixture(name) do
    @fixture_dir
    |> Path.join(name)
    |> File.read!()
    |> Jason.decode!()
  end

  defp put_paxeer_x_env(new_env) do
    initial_env = Application.get_env(:ethereum_jsonrpc, PaxeerX)

    on_exit(fn ->
      if is_nil(initial_env) do
        Application.delete_env(:ethereum_jsonrpc, PaxeerX)
      else
        Application.put_env(:ethereum_jsonrpc, PaxeerX, initial_env)
      end
    end)

    Application.put_env(:ethereum_jsonrpc, PaxeerX, new_env)
  end

  describe "Cosmos-originated transaction type 0xffffffff" do
    test "the wire marker is math.MaxUint32 and does not fit the transactions.type column" do
      assert PaxeerX.cosmos_transaction_type() == 4_294_967_295

      assert PaxeerX.cosmos_transaction_type() > 2_147_483_647,
             "the fixture only makes sense while the marker overflows a Postgres integer"

      assert PaxeerX.stored_cosmos_transaction_type() <= 2_147_483_647
    end

    test "the marker is recognised as a quantity and as a decoded integer" do
      assert PaxeerX.cosmos_transaction_type?("0xffffffff")
      assert PaxeerX.cosmos_transaction_type?(4_294_967_295)
      refute PaxeerX.cosmos_transaction_type?("0x2")
      refute PaxeerX.cosmos_transaction_type?(2)
      refute PaxeerX.cosmos_transaction_type?(nil)
    end

    test "the block body entry is parsed instead of raising on its null value" do
      %{"transaction" => transaction} = fixture("cosmos_originated_transaction.json")

      assert transaction["value"] == nil, "the encoder leaves value unset for a wasm execution"
      assert transaction["gasPrice"] == nil

      params =
        transaction
        |> Transaction.to_elixir()
        |> Transaction.elixir_to_params()

      assert params.value == 0
      assert params.gas_price == 0
      assert params.hash == transaction["hash"]
      assert params.block_number == 24_203_024
      assert params.index == 0
    end

    test "the shell receipt carries the marker onto the transaction it is merged into" do
      %{"transaction" => transaction, "receipt" => receipt} = fixture("cosmos_originated_transaction.json")

      assert receipt["type"] == "0xffffffff"

      transaction_params =
        transaction
        |> Transaction.to_elixir()
        |> Transaction.elixir_to_params()

      receipt_params =
        receipt
        |> Receipt.to_elixir()
        |> Receipt.elixir_to_params()

      assert receipt_params.type == PaxeerX.stored_cosmos_transaction_type()
      assert transaction_params.type == 0, "the block body renders these entries with a zero type"

      # `Indexer.Block.Fetcher.Receipts.put/2` merges receipt params over
      # transaction params, which is what makes the transaction recognisable.
      merged = Map.merge(transaction_params, receipt_params)

      assert merged.type == PaxeerX.stored_cosmos_transaction_type()
      assert merged.status == :ok
      assert merged.gas_used == 120_000
    end

    test "a transaction object stamped with the marker is normalized" do
      %{"transaction" => transaction} = fixture("cosmos_originated_transaction.json")

      params =
        transaction
        |> Map.put("type", "0xffffffff")
        |> Transaction.to_elixir()
        |> Transaction.elixir_to_params()

      assert params.type == PaxeerX.stored_cosmos_transaction_type()
    end

    test "an ordinary typed transaction keeps its type" do
      %{"transaction" => transaction} = fixture("cosmos_originated_transaction.json")

      params =
        transaction
        |> Map.merge(%{"type" => "0x2", "value" => "0xde0b6b3a7640000", "gasPrice" => "0x3b9aca00"})
        |> Transaction.to_elixir()
        |> Transaction.elixir_to_params()

      assert params.type == 2
      assert params.value == 1_000_000_000_000_000_000
    end

    test "the stored type is configurable" do
      put_paxeer_x_env(cosmos_transaction_type: 100)

      assert PaxeerX.stored_cosmos_transaction_type() == 100
      assert PaxeerX.normalize_transaction_type("0xffffffff") == 100
    end

    test "the stored type falls back to the documented default when unconfigured" do
      put_paxeer_x_env([])

      assert PaxeerX.stored_cosmos_transaction_type() == 0x7F
    end
  end

  describe "receiptsRoot" do
    test "is not verifiable and is imported verbatim even when two blocks share it" do
      refute PaxeerX.receipts_root_verifiable?()

      blocks = fixture("consecutive_blocks.json")
      lower = blocks["0x1714efd"]
      higher = blocks["0x1714efe"]

      assert higher["parentHash"] == lower["hash"]

      assert lower["receiptsRoot"] == higher["receiptsRoot"],
             "consecutive Paxeer X blocks share a LastResultsHash"

      %Blocks{blocks_params: blocks_params, errors: []} =
        Blocks.from_responses(
          [%{id: 0, result: lower}, %{id: 1, result: higher}],
          %{0 => %{number: 24_203_005}, 1 => %{number: 24_203_006}}
        )

      assert length(blocks_params) == 2

      for params <- blocks_params do
        assert params.receipts_root == higher["receiptsRoot"]
      end

      hashes = blocks_params |> Enum.map(& &1.hash) |> Enum.uniq()
      numbers = blocks_params |> Enum.map(& &1.number) |> Enum.sort()

      assert length(hashes) == 2
      assert numbers == [24_203_005, 24_203_006]
    end

    test "the root is the CometBFT LastResultsHash, not a receipts trie root" do
      higher = fixture("consecutive_blocks.json")["0x1714efe"]

      refute higher["receiptsRoot"] == "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
             "an empty Ethereum receipts trie would hash to the empty-root constant"

      assert higher["transactionsRoot"] == "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
    end
  end

  describe "newHeads" do
    setup do
      %{new_head: fixture("new_head.json"), block: fixture("consecutive_blocks.json")["0x1714efe"]}
    end

    test "the notification really does disagree with the body of the same height", %{
      new_head: new_head,
      block: block
    } do
      assert new_head["number"] == block["number"]
      assert new_head["hash"] == block["hash"]

      refute new_head["sha3Uncles"] == block["sha3Uncles"]
      refute new_head["transactionsRoot"] == block["transactionsRoot"]

      for key <- ~w(size totalDifficulty transactions uncles) do
        refute Map.has_key?(new_head, key), "the notification is a header, not a block body"
      end
    end

    test "the notification matches the body it announces", %{new_head: new_head, block: block} do
      %Blocks{blocks_params: [block_params]} =
        Blocks.from_responses([%{id: 0, result: block}], %{0 => %{number: 24_203_006}})

      assert PaxeerX.new_head_matches_block?(new_head, block_params)
    end

    test "a notification whose height disagrees with the body does not match", %{
      new_head: new_head,
      block: block
    } do
      %Blocks{blocks_params: [block_params]} =
        Blocks.from_responses([%{id: 0, result: block}], %{0 => %{number: 24_203_006}})

      refute PaxeerX.new_head_matches_block?(%{new_head | "number" => "0x1714eff"}, block_params)
    end

    test "the body fetched by hash is returned and the header is discarded", %{
      new_head: new_head,
      block: block,
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      hash = new_head["hash"]

      expect(EthereumJSONRPC.Mox, :json_rpc, fn [
                                                  %{
                                                    id: id,
                                                    method: "eth_getBlockByHash",
                                                    params: [^hash, true]
                                                  }
                                                ],
                                                _ ->
        {:ok, [%{id: id, result: block}]}
      end)

      assert {:ok, %Blocks{blocks_params: [block_params]}} =
               PaxeerX.fetch_block_by_new_head(new_head, json_rpc_named_arguments)

      assert block_params.hash == block["hash"]
      assert block_params.sha3_uncles == block["sha3Uncles"]
      assert block_params.transactions_root == block["transactionsRoot"]
      assert block_params.size == 920
    end

    test "an unknown hash is refetched by number", %{
      new_head: new_head,
      block: block,
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      hash = new_head["hash"]
      number_quantity = EthereumJSONRPC.integer_to_quantity(24_203_006)

      EthereumJSONRPC.Mox
      |> expect(:json_rpc, fn [%{id: id, method: "eth_getBlockByHash", params: [^hash, true]}], _ ->
        {:ok, [%{id: id, result: nil}]}
      end)
      |> expect(:json_rpc, fn [%{id: id, method: "eth_getBlockByNumber", params: [^number_quantity, true]}], _ ->
        {:ok, [%{id: id, result: block}]}
      end)

      assert {:ok, %Blocks{blocks_params: [block_params], errors: []}} =
               PaxeerX.fetch_block_by_new_head(new_head, json_rpc_named_arguments)

      assert block_params.number == 24_203_006
    end

    test "a body whose height disagrees with the notification is refetched by number", %{
      new_head: new_head,
      block: block,
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      hash = new_head["hash"]
      stale_block = Map.put(block, "number", "0x1714efd")
      number_quantity = EthereumJSONRPC.integer_to_quantity(24_203_006)

      EthereumJSONRPC.Mox
      |> expect(:json_rpc, fn [%{id: id, method: "eth_getBlockByHash", params: [^hash, true]}], _ ->
        {:ok, [%{id: id, result: stale_block}]}
      end)
      |> expect(:json_rpc, fn [%{id: id, method: "eth_getBlockByNumber", params: [^number_quantity, true]}], _ ->
        {:ok, [%{id: id, result: block}]}
      end)

      assert {:ok, %Blocks{blocks_params: [block_params]}} =
               PaxeerX.fetch_block_by_new_head(new_head, json_rpc_named_arguments)

      assert block_params.number == 24_203_006
    end

    test "a notification without a usable height is rejected", %{
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      assert {:error, {:invalid_new_head, :number}} =
               PaxeerX.fetch_block_by_new_head(%{"hash" => "0x0"}, json_rpc_named_arguments)
    end
  end

  describe "eth_syncing" do
    test "the node's -32000 refusal is reported as synced", %{
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      expect(EthereumJSONRPC.Mox, :json_rpc, fn %{method: "eth_syncing", params: []}, _ ->
        {:error, %{"code" => -32_000, "message" => "eth_syncing is not supported on Pax EVM RPC"}}
      end)

      assert {:ok, :synced} = PaxeerX.fetch_syncing_status(json_rpc_named_arguments)
    end

    test "a node that answers false is synced", %{json_rpc_named_arguments: json_rpc_named_arguments} do
      expect(EthereumJSONRPC.Mox, :json_rpc, fn %{method: "eth_syncing"}, _ -> {:ok, false} end)

      assert {:ok, :synced} = PaxeerX.fetch_syncing_status(json_rpc_named_arguments)
    end

    test "a node that reports progress is read normally", %{
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      expect(EthereumJSONRPC.Mox, :json_rpc, fn %{method: "eth_syncing"}, _ ->
        {:ok, %{"currentBlock" => "0x1714efd", "highestBlock" => "0x1714efe", "startingBlock" => "0x0"}}
      end)

      assert {:ok, {:syncing, 24_203_005, 24_203_006}} =
               PaxeerX.fetch_syncing_status(json_rpc_named_arguments)
    end

    test "an unrecognised answer is an error, not a silent synced", %{
      json_rpc_named_arguments: json_rpc_named_arguments
    } do
      expect(EthereumJSONRPC.Mox, :json_rpc, fn %{method: "eth_syncing"}, _ -> {:ok, "maybe"} end)

      assert {:error, {:unexpected_syncing_result, "maybe"}} =
               PaxeerX.fetch_syncing_status(json_rpc_named_arguments)
    end
  end

  describe "native coin decimals" do
    test "the bank denomination is 6 decimals scaled by 10^12 to the EVM's 18" do
      assert PaxeerX.bank_denom_decimals() == 6
      assert PaxeerX.bank_denom_scaling_factor() == 1_000_000_000_000
      assert PaxeerX.bank_amount_to_wei(1) == 1_000_000_000_000
      assert PaxeerX.bank_amount_to_wei(2_500_000) == 2_500_000_000_000_000_000
      assert PaxeerX.wei_to_bank_amount(2_500_000_000_000_000_000) == 2_500_000
      assert PaxeerX.wei_to_bank_amount(999_999_999_999) == 0
    end

    test "the UI-facing decimals default to the EVM's 18" do
      put_paxeer_x_env([])

      assert PaxeerX.native_coin_decimals() == 18
    end

    test "the UI-facing decimals are configurable" do
      put_paxeer_x_env(native_coin_decimals: 6)

      assert PaxeerX.native_coin_decimals() == 6
    end
  end

  describe "variant callbacks" do
    test "the tracing surface is the unmodified Geth one" do
      assert PaxeerX.fetch_beneficiaries(1..2, []) == :ignore

      for {function, arity} <- EthereumJSONRPC.Variant.behaviour_info(:callbacks) do
        assert function_exported?(PaxeerX, function, arity),
               "#{function}/#{arity} is missing from the Paxeer X variant"
      end
    end
  end
end
