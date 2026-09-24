defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusView do
  use BlockScoutWeb, :view

  alias Explorer.Chain.Transaction

  @doc """
  Renders the rung a transaction has reached on the settlement ladder, and why it sits there.
  """
  def render("status.json", %{transaction: %Transaction{} = transaction, status: status}) do
    {rung, reason} = rung_and_reason(status, transaction.block_number)

    %{
      "transaction_hash" => to_string(transaction.hash),
      "block_number" => transaction.block_number,
      "status" => to_string(rung),
      "reason" => reason
    }
  end

  defp rung_and_reason({rung, reason}, _block_number) when is_atom(rung) and is_binary(reason), do: {rung, reason}

  defp rung_and_reason(rung, block_number) when is_atom(rung), do: {rung, reason(rung, block_number)}

  defp reason(:pending, _block_number), do: "the transaction has no block yet"

  defp reason(:instant, block_number), do: "included in block #{block_number}"

  defp reason(:sealed, block_number),
    do: "block #{block_number} is covered by a submitted anchor checkpoint"

  defp reason(:final, block_number),
    do: "block #{block_number} is covered by a finalized anchor checkpoint"
end
