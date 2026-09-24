defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorControllerTest do
  use BlockScoutWeb.ConnCase

  alias BlockScoutWeb.PaxeerXTables

  describe "GET /api/v2/paxeer-x/anchors" do
    test "answers with an empty list while lx_anchors is absent", %{conn: conn} do
      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert response == %{"items" => [], "next_page_params" => nil}
    end

    test "answers with the anchors newest batch first", %{conn: conn} do
      PaxeerXTables.create_anchors()

      for batch_number <- 1..3, do: PaxeerXTables.insert_anchor(batch_number, <<batch_number::256>>)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert Enum.map(response["items"], & &1["batch_number"]) == [3, 2, 1]
      assert response["next_page_params"] == nil

      [latest | _] = response["items"]
      assert latest["checkpoint_id"] == "0x" <> Base.encode16(<<3::256>>, case: :lower)
      assert latest["status"] == "submitted"
    end

    test "paginates", %{conn: conn} do
      PaxeerXTables.create_anchors()

      for batch_number <- 1..51, do: PaxeerXTables.insert_anchor(batch_number, <<batch_number::256>>)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)

      assert Enum.count(response["items"]) == 50
      assert response["next_page_params"] == %{"batch_number" => 2}

      second_page = json_response(get(conn, "/api/v2/paxeer-x/anchors", response["next_page_params"]), 200)

      assert Enum.map(second_page["items"], & &1["batch_number"]) == [1]
      assert second_page["next_page_params"] == nil
    end
  end
end
