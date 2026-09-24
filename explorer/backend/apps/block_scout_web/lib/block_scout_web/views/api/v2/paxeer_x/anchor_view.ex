defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorView do
  @moduledoc """
  Renders anchor checkpoints, every column `lx_anchors` carries.
  """

  use BlockScoutWeb, :view

  def render("anchors.json", %{anchors: anchors, next_page_params: next_page_params}) do
    %{"items" => anchors, "next_page_params" => next_page_params}
  end
end
