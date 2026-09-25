defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorControllerTest do
  use BlockScoutWeb.ConnCase

  alias Explorer.Chain.PaxeerX.Anchor
  alias Explorer.Repo

  describe "GET /api/v2/paxeer-x/anchors" do
    test "answers with an empty list while no checkpoint has been anchored", %{conn: conn} do
      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert response == %{"items" => [], "next_page_params" => nil}
    end

    test "answers with the anchors newest batch first", %{conn: conn} do
      for batch_number <- 1..3, do: insert_anchor(batch_number)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert Enum.map(response["items"], & &1["batch_number"]) == [3, 2, 1]
      assert response["next_page_params"] == nil

      [latest | _rest] = response["items"]

      assert latest["checkpoint_id"] == checkpoint_id(3)
      assert latest["checkpoint_height"] == 300
      assert latest["sealed_height"] == 299
      assert String.starts_with?(latest["state_root"], "0x")
      assert latest["block_number"] == 30
      refute is_nil(latest["timestamp"])
    end

    test "carries the newest row of a batch that climbed the anchor ladder", %{conn: conn} do
      insert_anchor(7, status: :submitted, kernel_height: 700, sealed_height: 699)
      insert_anchor(7, status: :final, kernel_height: 700, sealed_height: 710, block_number: 90)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert [%{"batch_number" => 7, "sealed_height" => 710}] = response["items"]
    end

    test "leaves out the checkpoints a reorged block carried", %{conn: conn} do
      anchor = insert_anchor(11)

      anchor.block_hash
      |> block_by_hash()
      |> Ecto.Changeset.change(consensus: false)
      |> Repo.update!()

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert response["items"] == []
    end

    test "carries as null the heights and the state root a checkpoint log left out", %{conn: conn} do
      insert_anchor(21, kernel_height: nil, sealed_height: nil, state_root: nil)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert [anchor] = response["items"]
      assert anchor["checkpoint_height"] == nil
      assert anchor["sealed_height"] == nil
      assert anchor["state_root"] == nil
      assert anchor["batch_number"] == 21
    end

    test "paginates", %{conn: conn} do
      for batch_number <- 1..51, do: insert_anchor(batch_number)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert Enum.count(response["items"]) == 50
      assert response["next_page_params"] == %{"batch_number" => 2, "items_count" => 50}

      second_page = json_response(get(conn, "/api/v2/paxeer-x/anchors", response["next_page_params"]), 200)

      assert Enum.map(second_page["items"], & &1["batch_number"]) == [1]
      assert second_page["next_page_params"] == nil
    end
  end

  defp insert_anchor(batch_number, overrides \\ []) do
    overrides = Map.new(overrides)
    block = insert(:block, number: Map.get(overrides, :block_number, batch_number * 10))
    transaction = :transaction |> insert() |> with_block(block)

    attributes =
      Map.merge(
        %{
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
          status: :submitted,
          kernel_height: batch_number * 100,
          sealed_height: batch_number * 100 - 1
        },
        Map.drop(overrides, [:block_number])
      )

    %Anchor{}
    |> Anchor.changeset(attributes)
    |> Repo.insert!()
  end

  defp checkpoint_id(batch_number), do: "0x" <> Base.encode16(<<batch_number::256>>, case: :lower)

  defp block_by_hash(hash), do: Repo.get_by!(Explorer.Chain.Block, hash: hash)
end
