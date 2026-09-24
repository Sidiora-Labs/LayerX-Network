defmodule BlockScoutWeb.API.V2.PaxeerX.ReceiptController do
  @moduledoc """
  Publishes the kernel receipts the LayerX receipt logs carry.
  """

  use BlockScoutWeb, :controller

  import BlockScoutWeb.Chain, only: [next_page_params: 5, split_list_by_page: 1]
  import Explorer.PagingOptions, only: [default_paging_options: 0]

  alias Explorer.Chain.PaxeerX.UnifiedAccount

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  @doc """
  Handles GET requests to `/api/v2/paxeer-x/receipts`.

  Answers with a page of kernel receipts, newest receipt id first.
  """
  @spec receipts(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def receipts(conn, params) do
    {receipts, next_page} =
      params
      |> paging_key()
      |> UnifiedAccount.receipts(default_paging_options().page_size, @api_true)
      |> split_list_by_page()

    conn
    |> put_status(200)
    |> render(:receipts, %{
      receipts: receipts,
      next_page_params: next_page_params(next_page, receipts, params, false, &paging_params/1)
    })
  end

  @doc """
  Handles GET requests to `/api/v2/paxeer-x/receipts/:id`.
  """
  @spec receipt(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def receipt(conn, %{"id" => id} = _params) do
    case UnifiedAccount.receipt(id, @api_true) do
      {:ok, receipt} ->
        conn
        |> put_status(200)
        |> render(:receipt, %{receipt: receipt})

      :error ->
        {:error, :not_found}
    end
  end

  defp paging_key(%{"id" => id}) when is_binary(id), do: id

  defp paging_key(_params), do: nil

  defp paging_params(%{id: id}), do: %{id: to_string(id)}
end
