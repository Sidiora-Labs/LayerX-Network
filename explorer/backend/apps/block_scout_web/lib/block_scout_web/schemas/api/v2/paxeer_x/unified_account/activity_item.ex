defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.ActivityItem do
  @moduledoc """
  This module defines the schema for one item of the unified account activity feed.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.SettlementRung
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.AssetNullable
  alias Explorer.Chain.PaxeerX.CustodyEvent
  alias OpenApiSpex.Schema

  @chain_kinds ["transaction", "token_transfer"]

  OpenApiSpex.schema(%{
    title: "PaxeerXActivityItem",
    description:
      "One item of the feed that merges the chain's transactions and token transfers with the LayerX " <>
        "custody events, each naming the asset it moves, the side of the network it happened on and " <>
        "the rung it has reached on the settlement ladder.",
    type: :object,
    properties: %{
      kind: %Schema{
        type: :string,
        enum: @chain_kinds ++ Enum.map(CustodyEvent.kinds(), &to_string/1),
        nullable: false,
        description: "What the item is"
      },
      hash: General.FullHash,
      block_number: %Schema{type: :integer, nullable: false, description: "Block the item was recorded in"},
      status: SettlementRung,
      side: %Schema{
        type: :string,
        enum: ["chain", "kernel"],
        nullable: false,
        description: "Side of the network the item happened on"
      },
      timestamp: General.TimestampNullable,
      asset: AssetNullable,
      amount: General.IntegerStringNullable,
      counterparty: %Schema{
        type: :string,
        nullable: true,
        description: "The other party: an EVM address hash on the chain side, a kernel account on the kernel side"
      }
    },
    required: [:kind, :hash, :block_number, :status, :side, :timestamp, :asset, :amount, :counterparty],
    nullable: false,
    additionalProperties: false
  })
end
