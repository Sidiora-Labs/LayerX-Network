defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusControllerTest do
  use BlockScoutWeb.ConnCase

  alias Explorer.Chain.PaxeerX.Anchor
  alias Explorer.Repo

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

      assert response == %{
               "rung" => "pending",
               "block_number" => nil,
               "sealed_batch_number" => nil,
               "finalized_batch_number" => nil,
               "checkpoint_id" => nil
             }
    end

    test "a transaction in a block no checkpoint covers is instant", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["rung"] == "instant"
      assert response["block_number"] == 4_242
      assert response["sealed_batch_number"] == nil
      assert response["finalized_batch_number"] == nil
    end

    test "carries as null every anchor batch while no checkpoint has been anchored", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["sealed_batch_number"] == nil
      assert response["finalized_batch_number"] == nil
      assert response["checkpoint_id"] == nil
      assert response["block_number"] == 4_242
    end

    test "a transaction a submitted checkpoint seals is sealed", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      insert_anchor(19, :submitted, 4_300)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["rung"] == "sealed"
      assert response["sealed_batch_number"] == 19
      assert response["finalized_batch_number"] == nil
      assert response["checkpoint_id"] == checkpoint_id(19)
    end

    test "a transaction a finalized checkpoint covers is final", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      insert_anchor(18, :final, 4_250)
      insert_anchor(19, :submitted, 4_300)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)

      assert response["rung"] == "final"
      assert response["sealed_batch_number"] == 19
      assert response["finalized_batch_number"] == 18
      assert response["checkpoint_id"] == checkpoint_id(19)
    end
  end

  defp insert_anchor(batch_number, status, sealed_height) do
    block = insert(:block, number: 5_000 + batch_number)
    transaction = :transaction |> insert() |> with_block(block)

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: 0,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      batch_number: batch_number,
      checkpoint_id: checkpoint_id(batch_number),
      state_root: to_string(block_hash()),
      receipt_root: to_string(block_hash()),
      signers: 5,
      status: status,
      kernel_height: sealed_height,
      sealed_height: sealed_height
    }

    %Anchor{}
    |> Anchor.changeset(attributes)
    |> Repo.insert!()
  end

  defp checkpoint_id(batch_number), do: "0x" <> Base.encode16(<<batch_number::256>>, case: :lower)
end
