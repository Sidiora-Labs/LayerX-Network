defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.AssetNullable do
  @moduledoc """
  This module defines the schema for a nullable unified account asset.
  """
  require OpenApiSpex

  alias BlockScoutWeb.Schemas.API.V2.PaxeerX.UnifiedAccount.Asset
  alias BlockScoutWeb.Schemas.Helper

  OpenApiSpex.schema(
    Asset.schema()
    |> Helper.extend_schema(
      title: "PaxeerXAssetNullable",
      description: "An activity item that moves no asset carries none.",
      nullable: true
    )
  )
end
