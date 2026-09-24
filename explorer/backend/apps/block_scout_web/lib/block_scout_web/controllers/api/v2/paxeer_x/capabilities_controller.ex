defmodule BlockScoutWeb.API.V2.PaxeerX.CapabilitiesController do
  @moduledoc """
  Publishes which Paxeer X Network surfaces the connected node answers today.

  The booleans come from `Explorer.Chain.PaxeerX.Capabilities`, which probes the
  precompile addresses on a timer; `checked_at` is the moment of the last probe
  the node answered, or `null` while none has.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  alias Explorer.Chain.PaxeerX.Capabilities
  alias OpenApiSpex.Schema

  plug(OpenApiSpex.Plug.CastAndValidate, json_render_error_v2: true)

  tags(["paxeer-x"])

  operation(:capabilities,
    summary: "Get the live Paxeer X Network surfaces",
    description:
      "Returns one boolean per LayerX surface precompile — custody, anchor, exchange, bridge, launchpad — and " <>
        "whether the addr precompile answers getUnifiedAccount, as of the last probe of the connected node. " <>
        "A surface that the running chain does not carry yet reads as false.",
    parameters: base_params(),
    responses: [
      ok:
        {"Surfaces the connected node answers.", "application/json",
         %Schema{
           type: :object,
           properties: %{
             custody: %Schema{type: :boolean},
             anchor: %Schema{type: :boolean},
             exchange: %Schema{type: :boolean},
             bridge: %Schema{type: :boolean},
             launchpad: %Schema{type: :boolean},
             unified_account: %Schema{type: :boolean},
             checked_at: %Schema{type: :string, format: :"date-time", nullable: true}
           },
           required: [
             :custody,
             :anchor,
             :exchange,
             :bridge,
             :launchpad,
             :unified_account,
             :checked_at
           ]
         }},
      unprocessable_entity: JsonErrorResponse.response()
    ]
  )

  @doc """
    Function to handle GET requests to `/api/v2/paxeer-x/capabilities` endpoint.
  """
  @spec capabilities(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def capabilities(conn, _params) do
    %{checked_at: checked_at} = snapshot = Capabilities.all()

    conn
    |> put_status(200)
    |> json(%{
      "custody" => snapshot.custody,
      "anchor" => snapshot.anchor,
      "exchange" => snapshot.exchange,
      "bridge" => snapshot.bridge,
      "launchpad" => snapshot.launchpad,
      "unified_account" => snapshot.unified_account,
      "checked_at" => checked_at && DateTime.to_iso8601(checked_at)
    })
  end
end
