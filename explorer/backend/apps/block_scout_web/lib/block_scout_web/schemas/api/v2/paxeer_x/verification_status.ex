defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.VerificationStatus do
  @moduledoc """
  This module defines the schema for the rung a kernel receipt has reached on the LayerX
  verification lattice.
  """
  require OpenApiSpex

  alias Explorer.Chain.PaxeerX.Receipt

  OpenApiSpex.schema(%{
    title: "PaxeerXVerificationStatus",
    description: "The rung the receipt has reached on the LayerX kernel verification lattice.",
    type: :string,
    enum: Enum.map(Receipt.statuses(), &to_string/1),
    nullable: false
  })
end
