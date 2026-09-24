defmodule BlockScoutWeb.API.V2.PaxeerX.ReceiptControllerTest do
  use BlockScoutWeb.ConnCase

  alias BlockScoutWeb.PaxeerXTables

  describe "GET /api/v2/paxeer-x/receipts" do
    test "answers with an empty list while lx_receipts is absent", %{conn: conn} do
      response = json_response(get(conn, "/api/v2/paxeer-x/receipts"), 200)

      assert response == %{"items" => [], "next_page_params" => nil}
    end

    test "answers with the receipts newest id first", %{conn: conn} do
      PaxeerXTables.create_receipts()

      for index <- 1..3, do: PaxeerXTables.insert_receipt("receipt-#{index}", index)

      response = json_response(get(conn, "/api/v2/paxeer-x/receipts"), 200)

      assert Enum.map(response["items"], & &1["id"]) == ["receipt-3", "receipt-2", "receipt-1"]
      assert response["next_page_params"] == nil
    end

    test "paginates", %{conn: conn} do
      PaxeerXTables.create_receipts()

      for index <- 100..150, do: PaxeerXTables.insert_receipt("receipt-#{index}", index)

      response = json_response(get(conn, "/api/v2/paxeer-x/receipts"), 200)

      assert Enum.count(response["items"]) == 50
      assert response["next_page_params"] == %{"id" => "receipt-101"}

      second_page = json_response(get(conn, "/api/v2/paxeer-x/receipts", response["next_page_params"]), 200)

      assert Enum.map(second_page["items"], & &1["id"]) == ["receipt-100"]
      assert second_page["next_page_params"] == nil
    end
  end

  describe "GET /api/v2/paxeer-x/receipts/:id" do
    test "answers 404 while lx_receipts is absent", %{conn: conn} do
      assert %{"message" => "Not found"} = json_response(get(conn, "/api/v2/paxeer-x/receipts/receipt-1"), 404)
    end

    test "answers 404 for an id the kernel does not carry", %{conn: conn} do
      PaxeerXTables.create_receipts()
      PaxeerXTables.insert_receipt("receipt-1", 1)

      assert %{"message" => "Not found"} = json_response(get(conn, "/api/v2/paxeer-x/receipts/receipt-2"), 404)
    end

    test "answers with one receipt", %{conn: conn} do
      PaxeerXTables.create_receipts()
      PaxeerXTables.insert_receipt("receipt-1", 7)

      response = json_response(get(conn, "/api/v2/paxeer-x/receipts/receipt-1"), 200)

      assert response["id"] == "receipt-1"
      assert response["batch_number"] == 7
      assert response["kind"] == "activity"
    end
  end
end
