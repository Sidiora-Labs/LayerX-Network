defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.Capabilities do
  @moduledoc """
  This module defines the schema for the Paxeer X Network capability probe.
  """
  require OpenApiSpex

  alias OpenApiSpex.Schema

  OpenApiSpex.schema(%{
    title: "PaxeerXCapabilities",
    description:
      "One boolean per LayerX surface precompile as of the last probe of the connected node. " <>
        "A surface the running chain does not carry yet reads as false.",
    type: :object,
    properties: %{
      addr: %Schema{type: :boolean, nullable: false, description: "The addr precompile answers getUnifiedAccount"},
      custody: %Schema{type: :boolean, nullable: false, description: "The custody precompile answers"},
      anchor: %Schema{type: :boolean, nullable: false, description: "The anchor precompile answers"},
      exchange: %Schema{type: :boolean, nullable: false, description: "The exchange precompile answers"},
      bridge: %Schema{type: :boolean, nullable: false, description: "The bridge precompile answers"},
      launchpad: %Schema{type: :boolean, nullable: false, description: "The launchpad precompile answers"}
    },
    required: [:addr, :custody, :anchor, :exchange, :bridge, :launchpad],
    nullable: false,
    additionalProperties: false
  })
end
