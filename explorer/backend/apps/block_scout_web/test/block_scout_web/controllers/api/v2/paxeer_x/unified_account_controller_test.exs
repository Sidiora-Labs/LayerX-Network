defmodule BlockScoutWeb.API.V2.PaxeerX.UnifiedAccountControllerTest do
  use BlockScoutWeb.ConnCase

  alias Explorer.Chain.Address
  alias Explorer.Chain.PaxeerX.{AccountBinding, CustodyEvent}
  alias Explorer.Repo

  @pax_address "pax1005qwm6w5jj26zq8tsjs3eyp6my5d5fthrlqk3"
  @kernel_key "3f1a9b0c5d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8"
  @asset_id "0x" <> String.duplicate("0", 63) <> "1"

  describe "GET /api/v2/addresses/:address_hash_param/unified" do
    test "rejects a malformed address", %{conn: conn} do
      request = get(conn, "/api/v2/addresses/0xdeadbeef/unified")

      assert %{"message" => "Invalid parameter(s)"} = json_response(request, 422)
    end

    test "knows only the evm spelling while the address has never been bound", %{conn: conn} do
      address = insert(:address)
      checksummed = Address.checksum(address.hash)

      response = json_response(get(conn, "/api/v2/addresses/#{checksummed}/unified"), 200)

      assert response["identities"] == %{
               "evm" => checksummed,
               "pax" => nil,
               "did" => nil,
               "kernel_account" => nil
             }

      assert response["activity"] == []
    end

    test "answers with the four identities of a bound account", %{conn: conn} do
      address = insert(:address)

      insert_binding(address)

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert response["identities"] == %{
               "evm" => Address.checksum(address.hash),
               "pax" => @pax_address,
               "did" => did(),
               "kernel_account" => kernel_account()
             }
    end

    test "forgets the identities the newest log unbound", %{conn: conn} do
      address = insert(:address)
      block = insert(:block, number: 300)
      transaction = :transaction |> insert() |> with_block(block)

      insert_binding(address, block: block, transaction: transaction, log_index: 0, bound: true)
      insert_binding(address, block: block, transaction: transaction, log_index: 1, bound: false)

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert response["identities"]["pax"] == nil
      assert response["identities"]["kernel_account"] == nil
    end

    test "sums one total per asset out of its chain, custody and kernel parts", %{conn: conn} do
      address = insert(:address, fetched_coin_balance: 1_000)

      insert_binding(address)

      token = insert(:token)

      insert(:address_current_token_balance,
        address: address,
        token_contract_address_hash: token.contract_address_hash,
        value: 250
      )

      block = insert(:block, number: 400)
      transaction = :transaction |> insert() |> with_block(block)

      insert_custody_event(block, transaction,
        log_index: 0,
        kind: :custody_deposit,
        direction: :deposit,
        amount: Decimal.new(700),
        address_hash: to_string(address.hash),
        account: account_hash()
      )

      insert_custody_event(block, transaction,
        log_index: 1,
        kind: :custody_release,
        direction: :withdrawal,
        amount: Decimal.new(200),
        address_hash: to_string(address.hash),
        account: nil
      )

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      items = Map.new(response["balances"], &{&1["asset"]["id"], &1})

      native = items["native"]
      assert native["total"] == "1000"
      assert native["parts"] == %{"chain" => "1000", "custody" => "0", "kernel" => "0"}
      assert native["asset"]["decimals"] == 18

      token_item = items[Address.checksum(token.contract_address_hash)]
      assert token_item["total"] == "250"
      assert token_item["parts"]["chain"] == "250"
      assert token_item["asset"]["symbol"] == token.symbol

      custody_item = items[@asset_id]
      assert custody_item["parts"]["custody"] == "500"
      assert custody_item["parts"]["kernel"] == "700"
      assert custody_item["total"] == "1200"
      assert custody_item["asset"]["decimals"] == nil
    end

    test "merges transactions, token transfers and kernel events into one feed", %{conn: conn} do
      address = insert(:address)

      insert_binding(address)

      first_block = insert(:block, number: 100)
      second_block = insert(:block, number: 200)
      third_block = insert(:block, number: 300)

      :transaction |> insert(from_address: address) |> with_block(first_block)

      transfer_transaction = :transaction |> insert() |> with_block(second_block)

      insert(:token_transfer,
        transaction: transfer_transaction,
        block: second_block,
        block_number: second_block.number,
        from_address: address
      )

      event_transaction = :transaction |> insert() |> with_block(third_block)

      insert_custody_event(third_block, event_transaction,
        log_index: 7,
        kind: :custody_deposit,
        direction: :deposit,
        amount: Decimal.new(42),
        address_hash: to_string(address.hash),
        account: account_hash()
      )

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      items = response["activity"]

      assert Enum.map(items, & &1["kind"]) == ["custody_deposit", "token_transfer", "transaction"]
      assert Enum.map(items, & &1["block_number"]) == [300, 200, 100]
      assert Enum.map(items, & &1["side"]) == ["kernel", "chain", "chain"]
      assert Enum.all?(items, &(&1["status"] == "instant"))

      [event | _rest] = items

      assert event["amount"] == "42"
      assert event["asset"]["id"] == @asset_id
      assert event["hash"] == to_string(event_transaction.hash)
      assert event["counterparty"] == account_hash()
      refute is_nil(event["timestamp"])
    end

    test "carries as null the asset, the amount and the counterparty a kernel event left out", %{conn: conn} do
      address = insert(:address)
      block = insert(:block, number: 500)
      transaction = :transaction |> insert() |> with_block(block)

      insert_custody_event(block, transaction,
        log_index: 0,
        kind: :custody_release,
        direction: :withdrawal,
        amount: nil,
        asset_id: nil,
        address_hash: to_string(address.hash),
        account: nil
      )

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert [item] = response["activity"]
      assert item["kind"] == "custody_release"
      assert item["asset"] == nil
      assert item["amount"] == nil
      assert item["counterparty"] == nil
    end

    test "pages the activity feed on the block number and the index of the last item", %{conn: conn} do
      address = insert(:address)

      for number <- 1..51 do
        block = insert(:block, number: number)

        :transaction |> insert(from_address: address) |> with_block(block)
      end

      response = json_response(get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"), 200)

      assert Enum.count(response["activity"]) == 50
      assert List.last(response["activity"])["block_number"] == 2

      second_page =
        json_response(
          get(conn, "/api/v2/addresses/#{Address.checksum(address.hash)}/unified", %{
            "block_number" => "2",
            "index" => "0"
          }),
          200
        )

      assert Enum.map(second_page["activity"], & &1["block_number"]) == [1]
    end
  end

  defp insert_binding(address, options \\ []) do
    block = Keyword.get_lazy(options, :block, fn -> insert(:block, number: 10) end)
    transaction = Keyword.get_lazy(options, :transaction, fn -> :transaction |> insert() |> with_block(block) end)

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: Keyword.get(options, :log_index, 0),
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      evm_address_hash: to_string(address.hash),
      pax_address: @pax_address,
      layerx_did: did(),
      layerx_account: kernel_account(),
      bound: Keyword.get(options, :bound, true),
      nonce: 1
    }

    %AccountBinding{}
    |> AccountBinding.changeset(attributes)
    |> Repo.insert!()
  end

  defp insert_custody_event(block, transaction, fields) do
    attributes =
      Map.merge(
        %{
          transaction_hash: transaction.hash,
          block_hash: block.hash,
          block_number: block.number,
          block_consensus: true,
          asset_id: @asset_id
        },
        Map.new(fields)
      )

    %CustodyEvent{}
    |> CustodyEvent.changeset(attributes)
    |> Repo.insert!()
  end

  defp did, do: "did:layerx:" <> @kernel_key

  defp kernel_account, do: "agent:did:layerx:" <> @kernel_key <> ":main"

  defp account_hash, do: "0x" <> @kernel_key
end
