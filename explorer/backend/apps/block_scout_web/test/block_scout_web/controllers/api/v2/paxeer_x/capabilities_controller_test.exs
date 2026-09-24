defmodule BlockScoutWeb.API.V2.PaxeerX.CapabilitiesControllerTest do
  use BlockScoutWeb.ConnCase, async: false

  import Mox

  alias Explorer.Chain.PaxeerX.Capabilities

  @head "0x16c1320"

  # `EthereumJSONRPC.integer_to_quantity/1` spells the height back in uppercase
  # hexadecimal, which is what the probe requests carry.
  @head_quantity "0x16C1320"

  @code_addresses [
    "0x0000000000000000000000000000000000001013",
    "0x0000000000000000000000000000000000001014",
    "0x0000000000000000000000000000000000001015",
    "0x0000000000000000000000000000000000001016",
    "0x0000000000000000000000000000000000001017"
  ]

  @addr_precompile "0x0000000000000000000000000000000000001004"

  @unified_account_answer "0x" <> String.duplicate("0", 320)

  @bytecode "0x60806040523480156100105760006000fd"

  @surfaces ~w(addr custody anchor exchange bridge launchpad)

  describe "/api/v2/paxeer-x/capabilities" do
    test "reports every surface absent while nothing has probed the node", %{conn: conn} do
      response =
        conn
        |> get("/api/v2/paxeer-x/capabilities")
        |> json_response(200)

      for surface <- @surfaces do
        assert response[surface] == false
      end
    end

    test "reports the surfaces of the last probe", %{conn: conn} do
      set_mox_global()

      stub(EthereumJSONRPC.Mox, :json_rpc, fn
        %{method: "eth_blockNumber"}, _options ->
          {:ok, @head}

        requests, _options when is_list(requests) ->
          {:ok, Enum.map(requests, &answer/1)}
      end)

      start_supervised!(Capabilities)

      assert {:ok, _snapshot} = Capabilities.refresh()

      response =
        conn
        |> get("/api/v2/paxeer-x/capabilities")
        |> json_response(200)

      for surface <- @surfaces do
        assert response[surface] == true
      end
    end
  end

  defp answer(%{id: id, method: "eth_getCode", params: [address, block]}) do
    assert address in @code_addresses
    assert block == @head_quantity

    %{id: id, jsonrpc: "2.0", result: @bytecode}
  end

  defp answer(%{
         id: id,
         method: "eth_call",
         params: [%{to: @addr_precompile, data: _data}, block]
       }) do
    assert block == @head_quantity

    %{id: id, jsonrpc: "2.0", result: @unified_account_answer}
  end
end
