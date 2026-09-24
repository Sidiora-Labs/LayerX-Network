defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusView do
  @moduledoc """
  Renders the rung a transaction has reached on the Paxeer X Network settlement ladder, with
  the anchor batches the sealed and the final rungs were measured against.
  """

  use BlockScoutWeb, :view

  alias Explorer.Chain.PaxeerX.Finality.Anchor
  alias Explorer.Chain.PaxeerX.Status

  def render("status.json", %{status: %Status{} = status}) do
    %{
      "rung" => to_string(status.rung),
      "block_number" => status.block_number,
      "sealed_batch_number" => sealed_batch_number(status.anchor),
      "finalized_batch_number" => finalized_batch_number(status.anchor),
      "checkpoint_id" => checkpoint_id(status.anchor)
    }
  end

  defp sealed_batch_number(%Anchor{batch_number: batch_number}), do: batch_number
  defp sealed_batch_number(nil), do: nil

  defp finalized_batch_number(%Anchor{finalized_batch_number: batch_number}), do: batch_number
  defp finalized_batch_number(nil), do: nil

  defp checkpoint_id(%Anchor{checkpoint_id: checkpoint_id}), do: checkpoint_id
  defp checkpoint_id(nil), do: nil
end
