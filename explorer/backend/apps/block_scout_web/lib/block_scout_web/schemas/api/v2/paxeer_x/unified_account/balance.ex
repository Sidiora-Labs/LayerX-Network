defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.Balance do
  @moduledoc """
  This module defines the schema for one asset of a unified account, with the single total it
  carries beside the parts that total sums.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.Asset
  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXBalance",
    description:
      "One asset of the account. Amounts are raw units: the native coin in wei, a token in its own " <>
        "smallest unit and a custody asset in the unit its asset id is denominated in.",
    type: :object,
    properties: %{
      asset: Asset,
      total: General.IntegerString,
      parts: %Schema{
        type: :object,
        description: "The three sides the total sums: the chain, the custody events and the kernel account",
        properties: %{
          chain: General.IntegerString,
          custody: General.IntegerString,
          kernel: General.IntegerString
        },
        required: [:chain, :custody, :kernel],
        nullable: false,
        additionalProperties: false
      }
    },
    required: [:asset, :total, :parts],
    nullable: false,
    additionalProperties: false
  })
end
