defmodule BlockScoutWeb.API.V2.PaxeerX.ReceiptController do
  @moduledoc """
  Publishes the kernel receipts the LayerX receipt logs carry.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  import BlockScoutWeb.Chain, only: [next_page_params: 5, split_list_by_page: 1]
  import Explorer.PagingOptions, only: [default_paging_options: 0]

  alias BlockScoutWeb.Schemas.API.V2.ErrorResponses.NotFoundResponse
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.Parameters
  alias Explorer.Chain.PaxeerX.UnifiedAccount

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  tags(["paxeer-x"])

  operation :receipts,
    summary: "List the kernel receipts of Paxeer X Network",
    description:
      "Retrieves a paginated list of kernel receipts, newest receipt id first, one row per receipt: a " <>
        "receipt is logged again on every rung of its verification lattice, and the newest row of the " <>
        "receipt is the one the page carries.",
    parameters:
      base_params() ++
        Parameters.define_paging_params(["receipt_id"]) ++
        define_paging_params(["items_count"]),
    responses: [
      ok:
        {"Kernel receipts with pagination.", "application/json",
         paginated_response(
           items: Schemas.PaxeerX.Receipt,
           next_page_params_example: %{
             "id" => "0x0000000000000000000000000000000000000000000000000000000000000065",
             "items_count" => 50
           }
         )}
    ]

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

  operation :receipt,
    summary: "Retrieve one kernel receipt of Paxeer X Network",
    description:
      "Retrieves one kernel receipt by its id, as the newest log of that id reports it, with the rung " <>
        "it has reached on the kernel verification lattice and the log that recorded it.",
    parameters: [Parameters.receipt_id_param() | base_params()],
    responses: [
      ok: {"The kernel receipt.", "application/json", Schemas.PaxeerX.Receipt.Details},
      not_found: NotFoundResponse.response()
    ]

  @doc """
  Handles GET requests to `/api/v2/paxeer-x/receipts/:id`.
  """
  @spec receipt(Plug.Conn.t(), map()) :: Plug.Conn.t() | {atom(), any()}
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
