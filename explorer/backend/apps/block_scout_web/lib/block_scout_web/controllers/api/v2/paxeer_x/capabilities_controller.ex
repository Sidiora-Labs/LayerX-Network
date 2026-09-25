defmodule BlockScoutWeb.API.V2.PaxeerX.CapabilitiesController do
  @moduledoc """
  Publishes which Paxeer X Network surfaces the connected node answers today.

  The booleans come from `Explorer.Chain.PaxeerX.Capabilities`, which probes the
  precompile addresses on a timer. `addr` is the addr precompile, published
  under the name the unified surfaces read it by; a surface no probe has
  answered for yet reads as false.
  """

  use BlockScoutWeb, :controller
  use OpenApiSpex.ControllerSpecs

  alias Explorer.Chain.PaxeerX.Capabilities

  plug(OpenApiSpex.Plug.CastAndValidate, json_render_error_v2: true)

  tags(["paxeer-x"])

  operation(:capabilities,
    summary: "Get the live Paxeer X Network surfaces",
    description:
      "Returns one boolean per LayerX surface precompile — addr, custody, anchor, exchange, bridge, launchpad — " <>
        "as of the last probe of the connected node. `addr` is true when the addr precompile answers " <>
        "getUnifiedAccount. A surface that the running chain does not carry yet reads as false.",
    parameters: base_params(),
    responses: [
      ok: {"Surfaces the connected node answers.", "application/json", Schemas.PaxeerX.Capabilities},
      unprocessable_entity: JsonErrorResponse.response()
    ]
  )

  @doc """
    Function to handle GET requests to `/api/v2/paxeer-x/capabilities` endpoint.
  """
  @spec capabilities(Plug.Conn.t(), map()) :: Plug.Conn.t()
  def capabilities(conn, _params) do
    snapshot = Capabilities.all()

    conn
    |> put_status(200)
    |> json(%{
      "addr" => snapshot.unified_account,
      "custody" => snapshot.custody,
      "anchor" => snapshot.anchor,
      "exchange" => snapshot.exchange,
      "bridge" => snapshot.bridge,
      "launchpad" => snapshot.launchpad
    })
  end
end
