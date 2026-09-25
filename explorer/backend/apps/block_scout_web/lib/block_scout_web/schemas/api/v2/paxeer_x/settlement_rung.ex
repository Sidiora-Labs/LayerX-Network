defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.SettlementRung do
  @moduledoc """
  This module defines the schema for the rung an item sits on in the Paxeer X Network
  settlement ladder.
  """
  require OpenApiSpex

  OpenApiSpex.schema(%{
    title: "PaxeerXSettlementRung",
    description:
      "The rung the settlement ladder puts an item on: `pending` while no consensus block holds it, " <>
        "`instant` once one does, `sealed` once an anchor checkpoint seals its height, and `final` " <>
        "once a finalized checkpoint covers it.",
    type: :string,
    enum: ["pending", "instant", "sealed", "final"],
    nullable: false
  })
end
