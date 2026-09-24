defmodule BlockScoutWeb.API.V2.PaxeerX.ReceiptView do
  @moduledoc """
  Renders kernel receipts.

  A list item carries the receipt id, the kernel account it belongs to, the settlement rung of
  the block it was last seen in and that block's number. One receipt carries, on top of those,
  the rung it has reached on the kernel's own verification lattice and the log that recorded
  it.
  """

  use BlockScoutWeb, :view

  def render("receipts.json", %{receipts: receipts, next_page_params: next_page_params}) do
    %{"items" => Enum.map(receipts, &prepare_receipt/1), "next_page_params" => next_page_params}
  end

  def render("receipt.json", %{receipt: receipt}) do
    receipt
    |> prepare_receipt()
    |> Map.merge(%{
      "verification_status" => to_string(receipt.verification_status),
      "payload_hash" => receipt.payload_hash && to_string(receipt.payload_hash),
      "transaction_hash" => to_string(receipt.transaction_hash),
      "timestamp" => receipt.timestamp
    })
  end

  defp prepare_receipt(receipt) do
    %{
      "id" => to_string(receipt.id),
      "account" => receipt.account && to_string(receipt.account),
      "status" => to_string(receipt.status),
      "block_number" => receipt.block_number
    }
  end
end
