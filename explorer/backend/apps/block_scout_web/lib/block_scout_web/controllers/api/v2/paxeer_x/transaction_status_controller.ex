defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusController do
  @moduledoc """
  Publishes the rung a transaction has reached on the Paxeer X Network settlement ladder.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  alias BlockScoutWeb.Schemas.API.V2.ErrorResponses.NotFoundResponse
  alias Explorer.Chain
  alias Explorer.Chain.PaxeerX.UnifiedAccount

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  tags(["paxeer-x"])

  operation :status,
    summary: "Retrieve the settlement status of a transaction on Paxeer X Network",
    description:
      "Retrieves the rung the transaction has reached on the settlement ladder and the anchor batches " <>
        "that rung was measured against.",
    parameters: [transaction_hash_param() | base_params()],
    responses: [
      ok: {"The settlement status of the transaction.", "application/json", Schemas.PaxeerX.TransactionStatus},
      not_found: NotFoundResponse.response(),
      unprocessable_entity: {"Invalid parameter(s).", "application/json", message_response_schema()}
    ]

  @doc """
  Handles GET requests to `/api/v2/transactions/:transaction_hash_param/status`.

  Answers with the rung the transaction has reached and the anchor batches that rung was
  measured against.
  """
  @spec status(Plug.Conn.t(), map()) :: Plug.Conn.t() | {atom(), any()}
  def status(conn, %{"transaction_hash_param" => transaction_hash_string} = _params) do
    with {:format, {:ok, transaction_hash}} <- {:format, Chain.string_to_full_hash(transaction_hash_string)},
         {:ok, transaction} <- Chain.hash_to_transaction(transaction_hash, @api_true) do
      conn
      |> put_status(200)
      |> render(:status, %{status: UnifiedAccount.ladder_status(transaction.block_number, @api_true)})
    end
  end
end
