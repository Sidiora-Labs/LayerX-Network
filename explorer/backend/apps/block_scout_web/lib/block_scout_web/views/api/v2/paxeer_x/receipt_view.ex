defmodule BlockScoutWeb.API.V2.PaxeerX.ReceiptView do
  @moduledoc """
  Renders kernel receipts, every column `lx_receipts` carries.
  """

  use BlockScoutWeb, :view

  def render("receipts.json", %{receipts: receipts, next_page_params: next_page_params}) do
    %{"items" => receipts, "next_page_params" => next_page_params}
  end

  def render("receipt.json", %{receipt: receipt}), do: receipt
end
