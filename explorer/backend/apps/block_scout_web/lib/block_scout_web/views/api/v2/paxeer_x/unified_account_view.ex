defmodule BlockScoutWeb.API.V2.PaxeerX.UnifiedAccountView do
  @moduledoc """
  Renders the one-account view of the Paxeer X Network.

  `identities` carries the four spellings of the account, `balances` one row per asset with a
  single total beside the parts it sums, and `activity` one feed in which every item names the
  asset it moves, the side of the network it happened on and the rung it has reached on the
  settlement ladder.
  """

  use BlockScoutWeb, :view

  def render("unified.json", %{identities: identities, balances: balances, activity: activity}) do
    %{
      "identities" => %{
        "evm" => identities.evm,
        "pax" => identities.pax,
        "did" => identities.did,
        "kernel_account" => identities.kernel_account
      },
      "balances" => Enum.map(balances, &prepare_balance/1),
      "activity" => Enum.map(activity, &prepare_activity_item/1)
    }
  end

  defp prepare_balance(balance) do
    %{
      "asset" => prepare_asset(balance.asset),
      "total" => to_string(balance.total),
      "parts" => %{
        "chain" => to_string(balance.parts.chain),
        "custody" => to_string(balance.parts.custody),
        "kernel" => to_string(balance.parts.kernel)
      }
    }
  end

  defp prepare_asset(nil), do: nil

  defp prepare_asset(asset) do
    %{
      "id" => asset.id,
      "denom" => asset.denom,
      "symbol" => asset.symbol,
      "decimals" => asset.decimals
    }
  end

  defp prepare_activity_item(item) do
    %{
      "kind" => item.kind,
      "hash" => item.hash,
      "block_number" => item.block_number,
      "status" => to_string(item.status),
      "side" => to_string(item.side),
      "timestamp" => item.timestamp,
      "asset" => prepare_asset(item.asset),
      "amount" => item.amount && to_string(item.amount),
      "counterparty" => item.counterparty
    }
  end
end
