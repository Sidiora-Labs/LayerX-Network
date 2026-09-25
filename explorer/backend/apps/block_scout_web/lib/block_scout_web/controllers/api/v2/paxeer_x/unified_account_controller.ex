defmodule BlockScoutWeb.API.V2.PaxeerX.UnifiedAccountController do
  @moduledoc """
  Publishes the one-account view of the Paxeer X Network for one EVM address.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  import BlockScoutWeb.Chain, only: [paging_options: 1, split_list_by_page: 1]

  alias BlockScoutWeb.AccessHelper
  alias Explorer.{Chain, PagingOptions}
  alias Explorer.Chain.PaxeerX.UnifiedAccount

  action_fallback(BlockScoutWeb.API.V2.FallbackController)

  @api_true [api?: true]

  tags(["paxeer-x"])

  operation :unified,
    summary: "Retrieve the one-account view of an address on Paxeer X Network",
    description:
      "Retrieves the one-account view of an EVM address: the account's four identities, one asset list " <>
        "where each asset carries a single total beside its chain, custody and kernel parts, and one " <>
        "activity feed merging the chain's transactions and token transfers with the LayerX kernel " <>
        "events. The feed is keyed by the block number and the index of the last item it returned.",
    parameters:
      [address_hash_param() | base_params()] ++
        define_paging_params(["block_number", "index", "items_count"]),
    responses: [
      ok: {"The one-account view of the address.", "application/json", Schemas.PaxeerX.UnifiedAccount},
      forbidden: ForbiddenResponse.response(),
      unprocessable_entity: {"Invalid parameter(s).", "application/json", message_response_schema()}
    ]

  @doc """
  Handles GET requests to `/api/v2/addresses/:address_hash_param/unified`.

  Answers with the one-account view: the account's four identities, one asset list where each
  asset carries a single total beside its chain, custody and kernel parts, and one activity
  feed merging the chain's transactions and token transfers with the LayerX kernel events.
  """
  @spec unified(Plug.Conn.t(), map()) :: Plug.Conn.t() | {atom(), any()}
  def unified(conn, %{"address_hash_param" => address_hash_string} = params) do
    with {:format, {:ok, address_hash}} <- {:format, Chain.string_to_address_hash(address_hash_string)},
         {:ok, false} <- AccessHelper.restricted_access?(address_hash_string, params) do
      identities = UnifiedAccount.identities(address_hash, @api_true)
      balances = UnifiedAccount.balances(address_hash, identities.kernel_account, @api_true)

      [paging_options: %PagingOptions{page_size: page_size, key: key}] = paging_options(params)

      {activity, _next_page} =
        address_hash
        |> UnifiedAccount.activity(identities.kernel_account, activity_paging_key(key), page_size, @api_true)
        |> split_list_by_page()

      conn
      |> put_status(200)
      |> render(:unified, %{identities: identities, balances: balances, activity: activity})
    end
  end

  defp activity_paging_key({block_number, index}) when is_integer(block_number) and is_integer(index),
    do: {block_number, index}

  defp activity_paging_key(_key), do: nil
end
