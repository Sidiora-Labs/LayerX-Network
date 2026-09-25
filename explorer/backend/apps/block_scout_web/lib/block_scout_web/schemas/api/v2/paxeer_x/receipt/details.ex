defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.Receipt.Details do
  @moduledoc """
  This module defines the schema for one kernel receipt read on its own.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.General
  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.{Receipt, VerificationStatus}
  alias BlockScoutWeb.Schemas.Helper

  OpenApiSpex.schema(
    Receipt.schema()
    |> Helper.extend_schema(
      title: "PaxeerXReceiptDetails",
      description:
        "One kernel receipt with, on top of what a list item carries, the rung it has reached on the " <>
          "kernel verification lattice and the log that recorded it.",
      properties: %{
        verification_status: VerificationStatus,
        payload_hash: General.FullHashNullable,
        transaction_hash: General.FullHash,
        timestamp: General.Timestamp
      },
      required: [:verification_status, :payload_hash, :transaction_hash, :timestamp]
    )
  )
end
