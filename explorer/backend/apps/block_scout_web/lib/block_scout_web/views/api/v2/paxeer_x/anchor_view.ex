defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorView do
  @moduledoc """
  Renders anchor checkpoints: one row per batch, with the heights it records and seals, the
  state root it commits to and the block the checkpoint was observed in.
  """

  use BlockScoutWeb, :view

  def render("anchors.json", %{anchors: anchors, next_page_params: next_page_params}) do
    %{"items" => Enum.map(anchors, &prepare_anchor/1), "next_page_params" => next_page_params}
  end

  defp prepare_anchor(anchor) do
    %{
      "batch_number" => anchor.batch_number,
      "checkpoint_id" => to_string(anchor.checkpoint_id),
      "checkpoint_height" => anchor.checkpoint_height,
      "sealed_height" => anchor.sealed_height,
      "state_root" => anchor.state_root && to_string(anchor.state_root),
      "block_number" => anchor.block_number,
      "timestamp" => anchor.timestamp
    }
  end
end
