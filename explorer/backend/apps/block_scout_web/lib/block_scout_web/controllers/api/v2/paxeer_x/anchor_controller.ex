defmodule BlockScoutWeb.API.V2.PaxeerX.AnchorController do
  @moduledoc """
  Publishes the anchor checkpoints the LayerX anchor precompile has logged.
  """

  use BlockScoutWeb, :controller

  import BlockScoutWeb.Chain, only: [next_page_params: 5, split_list_by_page: 1]
  import Explorer.PagingOptions, only: [default_paging_options: 0]

  alias Explorer.Chain.PaxeerX.UnifiedAccount
  alias Explorer.Helper, as: ExplorerHelper

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

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
