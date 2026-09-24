defmodule BlockScoutWeb.API.V2.PaxeerX.UnifiedAccountControllerTest do
  use BlockScoutWeb.ConnCase

  alias BlockScoutWeb.PaxeerXTables
  alias Explorer.Chain.Address

  @pax_address "pax1005qwm6w5jj26zq8tsjs3eyp6my5d5fthrlqk3"
  @kernel_key "3f1a9b0c5d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8"
  @asset_id <<1::256>>

  describe "GET /api/v2/addresses/:address_hash_param/unified" do
    test "rejects a malformed address", %{conn: conn} do
      request = get(conn, "/api/v2/addresses/0xdeadbeef/unified")

      assert %{"message" => "Invalid parameter(s)"} = json_response(request, 422)
    end

    test "knows only the evm spelling while lx_account_bindings is absent", %{conn: conn} do
      address = insert(:address)
      checksummed = Address.checksum(address.hash)

      response = json_response(get(conn, "/api/v2/addresses/#{checksummed}/unified"), 200)

      assert response["requested"] == checksummed
      assert response["canonical"] == checksummed

      assert response["identities"] == %{
               "evm" => checksummed,
               "pax" => nil,
               "did" => nil,
               "kernel_account" => nil,
               "bound" => false
             }
    end

    test "answers with the four identities of a bound account", %{conn: conn} do
      PaxeerXTables.create_all()

      address = insert(:address)
      did = "did:layerx:" <> @kernel_key
      kernel_account = "agent:" <> did <> ":main"

      PaxeerXTables.insert_binding(address.hash.bytes, @pax_address, did, kernel_account)

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert response["identities"] == %{
               "evm" => Address.checksum(address.hash),
               "pax" => @pax_address,
               "did" => did,
               "kernel_account" => kernel_account,
               "bound" => true
             }

      assert response["canonical"] == kernel_account
    end

    test "sums one total per asset out of its chain, custody and kernel parts", %{conn: conn} do
      PaxeerXTables.create_all()

      address = insert(:address, fetched_coin_balance: 1_000)
      kernel_account = "agent:did:layerx:" <> @kernel_key <> ":main"

      PaxeerXTables.insert_binding(address.hash.bytes, @pax_address, "did:layerx:" <> @kernel_key, kernel_account)

      token = insert(:token)

      insert(:address_current_token_balance,
        address: address,
        token_contract_address_hash: token.contract_address_hash,
        value: 250
      )

      transaction = insert(:transaction)

      PaxeerXTables.insert_custody_event(
        event_type: "custody-deposit",
        asset_id: @asset_id,
        amount: Decimal.new(700),
        evm_address: address.hash.bytes,
        kernel_account: kernel_account,
        block_number: 10,
        log_index: 0,
        transaction_hash: transaction.hash.bytes
      )

      PaxeerXTables.insert_custody_event(
        event_type: "custody-release",
        asset_id: @asset_id,
        amount: Decimal.new(200),
        evm_address: address.hash.bytes,
        kernel_account: nil,
        block_number: 11,
        log_index: 0,
        transaction_hash: transaction.hash.bytes
      )

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      items = Map.new(response["balances"]["items"], &{&1["asset_id"], &1})

      native = items["native"]
      assert native["total"] == "1000"
      assert native["parts"] == %{"chain" => "1000", "custody" => "0", "kernel" => "0"}

      token_item = items[Address.checksum(token.contract_address_hash)]
      assert token_item["total"] == "250"
      assert token_item["parts"]["chain"] == "250"
      assert token_item["token"]["symbol"] == token.symbol

      custody_item = items["0x" <> Base.encode16(@asset_id, case: :lower)]
      assert custody_item["parts"]["custody"] == "500"
      assert custody_item["parts"]["kernel"] == "700"
      assert custody_item["total"] == "1200"
    end

    test "merges transactions, token transfers and kernel events into one feed", %{conn: conn} do
      PaxeerXTables.create_all()

      address = insert(:address)
      kernel_account = "agent:did:layerx:" <> @kernel_key <> ":main"

      PaxeerXTables.insert_binding(address.hash.bytes, @pax_address, "did:layerx:" <> @kernel_key, kernel_account)

      first_block = insert(:block, number: 100)
      second_block = insert(:block, number: 200)

      transaction = :transaction |> insert(from_address: address) |> with_block(first_block)

      transfer_transaction = :transaction |> insert() |> with_block(second_block)

      insert(:token_transfer,
        transaction: transfer_transaction,
        block: second_block,
        block_number: second_block.number,
        from_address: address
      )

      PaxeerXTables.insert_custody_event(
        event_type: "custody-deposit",
        asset_id: @asset_id,
        amount: Decimal.new(42),
        evm_address: address.hash.bytes,
        kernel_account: kernel_account,
        block_number: 300,
        log_index: 7,
        transaction_hash: transaction.hash.bytes
      )

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      items = response["activity"]["items"]

      assert Enum.map(items, & &1["kind"]) == ["custody-deposit", "token-transfer", "transaction"]
      assert Enum.map(items, & &1["block_number"]) == [300, 200, 100]
      assert Enum.all?(items, &(&1["status"] == "instant"))
      assert response["activity"]["next_page_params"] == nil

      [event | _] = items
      assert event["value"] == "42"
      assert event["asset"] == "0x" <> Base.encode16(@asset_id, case: :lower)
      assert event["index"] == 7
    end

    test "paginates the activity feed", %{conn: conn} do
      address = insert(:address)

      blocks = for number <- 1..51, do: insert(:block, number: number)

      for block <- blocks do
        :transaction |> insert(from_address: address) |> with_block(block)
      end

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert Enum.count(response["activity"]["items"]) == 50
      next_page_params = response["activity"]["next_page_params"]
      assert next_page_params["block_number"] == 2

      second_page =
        json_response(
          get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified", next_page_params),
          200
        )

      assert Enum.count(second_page["activity"]["items"]) == 1
      assert second_page["activity"]["next_page_params"] == nil
    end
  end
end
