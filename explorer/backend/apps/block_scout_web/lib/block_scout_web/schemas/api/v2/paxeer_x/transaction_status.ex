defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.TransactionStatus do
  @moduledoc """
  This module defines the schema for the rung a transaction has reached on the Paxeer X Network
  settlement ladder.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.SettlementRung
  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXTransactionStatus",
    description:
      "The rung a transaction has reached on the settlement ladder, with the anchor batches the " <>
        "sealed and the final rungs were measured against. A batch no anchor reports is null rather " <>
        "than zero.",
    type: :object,
    properties: %{
      rung: SettlementRung,
      block_number: %Schema{
        type: :integer,
        nullable: true,
        description: "Block holding the transaction, null while no block holds it"
      },
      sealed_batch_number: %Schema{
        type: :integer,
        nullable: true,
        description: "Batch of the latest checkpoint the anchor reports as sealing"
      },
      finalized_batch_number: %Schema{
        type: :integer,
        nullable: true,
        description: "Batch of the latest checkpoint the anchor reports as finalized"
      },
      checkpoint_id: General.FullHashNullable
    },
    required: [:rung, :block_number, :sealed_batch_number, :finalized_batch_number, :checkpoint_id],
    nullable: false,
    additionalProperties: false
  })
end
