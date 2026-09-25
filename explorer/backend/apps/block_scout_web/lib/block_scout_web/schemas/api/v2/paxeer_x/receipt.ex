defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.Receipt do
  @moduledoc """
  This module defines the schema for one kernel receipt in a list of them.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.SettlementRung
  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXReceipt",
    description:
      "One kernel receipt as the newest log of its id reports it: the receipt id, the kernel account " <>
        "it belongs to and the settlement rung of the block it was last seen in.",
    type: :object,
    properties: %{
      id: General.FullHash,
      account: General.FullHashNullable,
      status: SettlementRung,
      block_number: %Schema{type: :integer, nullable: false, description: "Block the receipt was last seen in"}
    },
    required: [:id, :account, :status, :block_number],
    nullable: false,
    additionalProperties: false
  })
end
