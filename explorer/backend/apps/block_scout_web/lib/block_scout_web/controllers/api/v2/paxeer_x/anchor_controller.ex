defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorController do
  @moduledoc """
  Publishes the anchor checkpoints the LayerX anchor precompile has logged.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  import BlockScoutWeb.Chain, only: [next_page_params: 5, split_list_by_page: 1]
  import Explorer.PagingOptions, only: [default_paging_options: 0]

  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.Parameters
  alias Explorer.Chain.PaxeerX.UnifiedAccount
  alias Explorer.Helper, as: ExplorerHelper

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  tags(["paxeer-x"])

  operation :anchors,
    summary: "List the anchor checkpoints of Paxeer X Network",
    description:
      "Retrieves a paginated list of anchor checkpoints, newest batch first, one row per batch: a " <>
        "checkpoint is logged again every time it climbs a rung of the anchor ladder, and the newest " <>
        "row of the batch is the one the page carries.",
    parameters:
      base_params() ++
        Parameters.define_paging_params(["batch_number"]) ++
        define_paging_params(["items_count"]),
    responses: [
      ok:
        {"Anchor checkpoints with pagination.", "application/json",
         paginated_response(
           items: Schemas.PaxeerX.Anchor,
           next_page_params_example: %{"batch_number" => 2, "items_count" => 50}
         )}
    ]

  @doc """
  Handles GET requests to `/api/v2/paxeer-x/anchors`.

  Answers with a page of anchor checkpoints, newest batch first.
  """
  @spec anchors(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def anchors(conn, params) do
    {anchors, next_page} =
      params
      |> paging_key()
      |> UnifiedAccount.anchors(default_paging_options().page_size, @api_true)
      |> split_list_by_page()

    conn
    |> put_status(200)
    |> render(:anchors, %{
      anchors: anchors,
      next_page_params: next_page_params(next_page, anchors, params, false, &paging_params/1)
    })
  end

  defp paging_key(%{"batch_number" => batch_number}) do
    case ExplorerHelper.safe_parse_non_negative_integer(batch_number) do
      {:ok, parsed} -> parsed
      _ -> nil
    end
  end

  defp paging_key(_params), do: nil

  defp paging_params(%{batch_number: batch_number}), do: %{batch_number: batch_number}
end
