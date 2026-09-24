defmodule Explorer.Chain.PaxeerX.CapabilitiesTest do
  use Explorer.DataCase, async: false

  import Mox

  alias Explorer.Chain.PaxeerX.Capabilities

  @head "0x16c1320"

  # `EthereumJSONRPC.integer_to_quantity/1` spells the height back in uppercase
  # hexadecimal, which is what the probe requests carry.
  @head_quantity "0x16C1320"

  @code_addresses %{
    "0x0000000000000000000000000000000000001013" => :custody,
    "0x0000000000000000000000000000000000001014" => :anchor,
    "0x0000000000000000000000000000000000001015" => :exchange,
    "0x0000000000000000000000000000000000001016" => :bridge,
    "0x0000000000000000000000000000000000001017" => :launchpad
  }

  @addr_precompile "0x0000000000000000000000000000000000001004"
  @get_unified_account "0x357feed6" <> String.duplicate("0", 64)

  # `(address, string, bytes32, bytes32)` as the addr precompile returns it for
  # an account with no binding: four head words, then the empty `paxAddr`.
  @unified_account_answer "0x" <>
                            String.duplicate("0", 64) <>
                            String.duplicate("0", 62) <>
                            "80" <>
                            String.duplicate("0", 64) <>
                            String.duplicate("0", 64) <> String.duplicate("0", 64)

  @bytecode "0x60806040523480156100105760006000fd"

  setup :verify_on_exit!

  describe "probe/1" do
    test "reads the surfaces the chain does not carry yet as absent" do
      expect_probe("0x", %{error: %{code: -32_000, message: "execution reverted"}})

      assert {:ok, snapshot} = Capabilities.probe(json_rpc_named_arguments())

      assert %{
               custody: false,
               anchor: false,
               exchange: false,
               bridge: false,
               launchpad: false,
               unified_account: false
             } = snapshot

      assert %DateTime{} = snapshot.checked_at
    end

    test "reads a surface with code and an answering addr method as live" do
      expect_probe(@bytecode, %{result: @unified_account_answer})

      assert {:ok, snapshot} = Capabilities.probe(json_rpc_named_arguments())

      assert %{
               custody: true,
               anchor: true,
               exchange: true,
               bridge: true,
               launchpad: true,
               unified_account: true
             } = snapshot
    end

    test "reads an addr answer shorter than the declared layout as absent" do
      expect_probe(@bytecode, %{result: "0x" <> String.duplicate("0", 64)})

      assert {:ok, %{custody: true, unified_account: false}} =
               Capabilities.probe(json_rpc_named_arguments())
    end

    test "refuses an answer that is not a code string" do
      EthereumJSONRPC.Mox
      |> expect(:json_rpc, fn %{method: "eth_blockNumber"}, _options -> {:ok, @head} end)
      |> expect(:json_rpc, fn requests, _options when is_list(requests) ->
        {:ok, Enum.map(requests, &answer(&1, "0xzz", %{result: @unified_account_answer}))}
      end)

      assert {:error, {:custody, {:unexpected_code, "zz"}}} =
               Capabilities.probe(json_rpc_named_arguments())
    end

    test "passes the node's refusal of the head back to the caller" do
      expect(EthereumJSONRPC.Mox, :json_rpc, fn %{method: "eth_blockNumber"}, _options ->
        {:error, :econnrefused}
      end)

      assert {:error, :econnrefused} = Capabilities.probe(json_rpc_named_arguments())
    end
  end

  describe "live?/1" do
    test "answers false for every surface while no probe has succeeded" do
      for surface <- Capabilities.surfaces() do
        refute Capabilities.live?(surface)
      end

      assert Capabilities.all().checked_at == nil
    end

    test "answers from the probe the process stored" do
      set_mox_global()

      stub(EthereumJSONRPC.Mox, :json_rpc, fn
        %{method: "eth_blockNumber"}, _options ->
          {:ok, @head}

        requests, _options when is_list(requests) ->
          {:ok, Enum.map(requests, &answer(&1, @bytecode, %{result: @unified_account_answer}))}
      end)

      start_supervised!(Capabilities)

      assert {:ok, _snapshot} = Capabilities.refresh()

      for surface <- Capabilities.surfaces() do
        assert Capabilities.live?(surface)
      end

      assert %DateTime{} = Capabilities.all().checked_at
    end

    test "keeps the stored probe when the node stops answering" do
      set_mox_global()
      test_process = self()

      stub(EthereumJSONRPC.Mox, :json_rpc, fn
        %{method: "eth_blockNumber"}, _options ->
          if Process.get(:probed) do
            {:error, :econnrefused}
          else
            Process.put(:probed, true)
            send(test_process, :probed)
            {:ok, @head}
          end

        requests, _options when is_list(requests) ->
          {:ok, Enum.map(requests, &answer(&1, @bytecode, %{result: @unified_account_answer}))}
      end)

      start_supervised!(Capabilities)

      assert_receive :probed

      assert {:error, :econnrefused} = Capabilities.refresh()
      assert Capabilities.live?(:custody)
    end
  end

  describe "address/1" do
    test "names the precompile every surface is probed at" do
      for {address, surface} <- @code_addresses do
        assert Capabilities.address(surface) == address
      end

      assert Capabilities.address(:unified_account) == @addr_precompile
    end
  end

  defp expect_probe(code, unified_account_answer) do
    EthereumJSONRPC.Mox
    |> expect(:json_rpc, fn %{id: _, method: "eth_blockNumber", params: []}, _options ->
      {:ok, @head}
    end)
    |> expect(:json_rpc, fn requests, _options when is_list(requests) ->
      assert length(requests) == map_size(@code_addresses) + 1

      {:ok, Enum.map(requests, &answer(&1, code, unified_account_answer))}
    end)
  end

  defp answer(
         %{id: id, method: "eth_getCode", params: [address, block]},
         code,
         _unified_account_answer
       ) do
    assert Map.has_key?(@code_addresses, address)
    assert block == @head_quantity

    %{id: id, jsonrpc: "2.0", result: code}
  end

  defp answer(
         %{id: id, method: "eth_call", params: [%{to: to, data: data}, block]},
         _code,
         unified_account_answer
       ) do
    assert to == @addr_precompile
    assert data == @get_unified_account
    assert block == @head_quantity

    unified_account_answer
    |> Map.put(:id, id)
    |> Map.put(:jsonrpc, "2.0")
  end

  defp json_rpc_named_arguments, do: Application.get_env(:explorer, :json_rpc_named_arguments)
end
