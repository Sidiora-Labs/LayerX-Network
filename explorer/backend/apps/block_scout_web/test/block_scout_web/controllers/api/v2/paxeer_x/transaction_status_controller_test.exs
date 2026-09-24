defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusControllerTest do
  use BlockScoutWeb.ConnCase

  describe "GET /api/v2/transactions/:transaction_hash_param/status" do
    test "rejects a malformed hash", %{conn: conn} do
      request = get(conn, "/api/v2/transactions/0x01/status")

      assert %{"message" => "Invalid parameter(s)"} = json_response(request, 422)
    end

    test "answers 404 for a hash the chain does not carry", %{conn: conn} do
      hash = to_string(build(:transaction).hash)

      request = get(conn, "/api/v2/transactions/#{hash}/status")

      assert %{"message" => "Not found"} = json_response(request, 404)
    end

    test "a transaction without a block is pending", %{conn: conn} do
      transaction = insert(:transaction)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["transaction_hash"] == to_string(transaction.hash)
      assert response["block_number"] == nil
      assert response["status"] == "pending"
      assert response["reason"] == "the transaction has no block yet"
    end

    test "a transaction in a block is instant", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["block_number"] == 4_242
      assert response["status"] == "instant"
      assert response["reason"] == "included in block 4242"
    end
  end
end
