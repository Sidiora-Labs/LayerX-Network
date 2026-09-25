defmodule BlockScoutWeb.Schemas.API.V2.PaxeerX.Parameters do
  @moduledoc """
  The request parameters the Paxeer X Network API paths carry and no other path does.

  `BlockScoutWeb.Schemas.API.V2.General` owns every parameter the rest of the API shares; the
  anchor batch number, the kernel receipt id in the path and the kernel receipt id the receipt
  page is keyed by live here.
  """

  alias BlockScoutWeb.Schemas.API.V2.General
  alias OpenApiSpex.{Parameter, Schema}

  @paging_params %{
    "batch_number" => %Parameter{
      name: :batch_number,
      in: :query,
      schema: %Schema{type: :integer, minimum: 0},
      required: false,
      description: "Anchor batch number for paging"
    },
    "receipt_id" => %Parameter{
      name: :id,
      in: :query,
      schema: General.FullHash,
      required: false,
      description: "Kernel receipt id for paging"
    }
  }

  @receipt_id_param %Parameter{
    name: :id,
    in: :path,
    schema: General.FullHash,
    required: true,
    description: "Kernel receipt id in the path"
  }

  @doc """
  Returns a parameter definition for a kernel receipt id in the path.
  """
  @spec receipt_id_param() :: Parameter.t()
  def receipt_id_param, do: @receipt_id_param

  @doc """
  Returns a list of Paxeer X paging parameters based on the provided field names.
  """
  @spec define_paging_params([String.t()]) :: [Parameter.t()]
  def define_paging_params(fields) do
    Enum.map(fields, fn field ->
      Map.get(@paging_params, field) || raise "Unknown Paxeer X paging param: #{field}"
    end)
  end
end
