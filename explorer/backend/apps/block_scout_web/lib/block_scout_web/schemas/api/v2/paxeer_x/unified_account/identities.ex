defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.Identities do
  @moduledoc """
  This module defines the schema for the four spellings of a unified account.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXIdentities",
    description:
      "The four spellings of the account. Only the EVM spelling is known while no consensus block " <>
        "has bound the address, or while the newest binding log unbound it.",
    type: :object,
    properties: %{
      evm: General.AddressHash,
      pax: %Schema{type: :string, nullable: true, description: "Bech32 pax address the account is bound to"},
      did: %Schema{type: :string, nullable: true, description: "LayerX decentralised identifier of the account"},
      kernel_account: %Schema{type: :string, nullable: true, description: "LayerX kernel account id"}
    },
    required: [:evm, :pax, :did, :kernel_account],
    nullable: false,
    additionalProperties: false
  })
end
