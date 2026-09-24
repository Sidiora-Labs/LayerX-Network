defmodule BlockScoutWeb.API.V2.PaxeerX.TransactionStatusController do
  use BlockScoutWeb, :controller

  alias Explorer.Chain
  alias Explorer.Chain.PaxeerX.UnifiedAccount

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  @doc """
  Handles GET requests to `/api/v2/transactions/:transaction_hash_param/status`.

  Answers with the rung the transaction has reached on the settlement ladder and the reason it
  sits there.
  """
  @spec status(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def status(conn, %{"transaction_hash_param" => transaction_hash_string} = _params) do
    with {:format, {:ok, transaction_hash}} <- {:format, Chain.string_to_full_hash(transaction_hash_string)},
         {:ok, transaction} <- Chain.hash_to_transaction(transaction_hash, @api_true) do
      conn
      |> put_status(200)
      |> render(:status, %{
        transaction: transaction,
        status: UnifiedAccount.ladder_status(transaction.block_number, @api_true)
      })
    end
  end
end
