defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.Asset do
  @moduledoc """
  This module defines the schema for the asset an amount of the unified account is denominated
  in.
  """
  require OpenApiSpex

  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXAsset",
    description:
      "The asset an amount is denominated in: the native coin, a token contract on the chain or a " <>
        "custody asset of the LayerX kernel. Only a token the explorer has read metadata for carries " <>
        "a symbol and a decimals count.",
    type: :object,
    properties: %{
      id: %Schema{
        type: :string,
        nullable: false,
        description: "`native` for the coin, the token contract address hash, or the kernel asset id"
      },
      denom: %Schema{type: :string, nullable: false, description: "Name the amount is quoted in"},
      symbol: %Schema{type: :string, nullable: true, description: "Token symbol, null while none is known"},
      decimals: %Schema{type: :integer, nullable: true, description: "Token decimals, null while none are known"}
    },
    required: [:id, :denom, :symbol, :decimals],
    nullable: false,
    additionalProperties: false
  })
end
