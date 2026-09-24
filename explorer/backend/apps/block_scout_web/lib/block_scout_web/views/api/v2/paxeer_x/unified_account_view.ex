defmodule BlockScoutWeb.API.V2.PaxeerX.UnifiedAccountView do
  use BlockScoutWeb, :view

  @doc """
  Renders the one-account view.

  `identities` carries the four spellings of the account, `balances` one row per asset with a
  single total beside the parts it sums, and `activity` one feed in which every item states the
  kind of thing it is and the rung it has reached on the settlement ladder.
  """
  def render("unified.json", %{
        requested: requested,
        identities: identities,
        balances: balances,
        activity: activity,
        next_page_params: next_page_params
      }) do
    %{
      "requested" => requested,
      "canonical" => canonical(identities),
      "identities" => %{
        "evm" => identities.evm,
        "pax" => identities.pax,
        "did" => identities.did,
        "kernel_account" => identities.kernel_account,
        "bound" => identities.bound
      },
      "balances" => %{"items" => Enum.map(balances, &prepare_asset/1)},
      "activity" => %{
        "items" => Enum.map(activity, &prepare_activity_item/1),
        "next_page_params" => next_page_params
      }
    }
  end

  defp canonical(%{kernel_account: kernel_account}) when is_binary(kernel_account), do: kernel_account
  defp canonical(%{evm: evm}), do: evm

  defp prepare_asset(asset) do
    %{
      "asset_id" => asset.asset_id,
      "denom" => asset.denom,
      "decimals" => asset.decimals && to_string(asset.decimals),
      "token" => asset.token,
      "total" => to_string(asset.total),
      "parts" => %{
        "chain" => to_string(asset.parts.chain),
        "custody" => to_string(asset.parts.custody),
        "kernel" => to_string(asset.parts.kernel)
      }
    }
  end

  defp prepare_activity_item(item) do
    %{
      "kind" => item.kind,
      "status" => to_string(item.status),
      "block_number" => item.block_number,
      "index" => item.ordinal,
      "timestamp" => item.timestamp,
      "transaction_hash" => item.transaction_hash,
      "from" => item.from,
      "to" => item.to,
      "value" => item.value && to_string(item.value),
      "asset" => item.asset
    }
  end
end
