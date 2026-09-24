defmodule Explorer.Chain.PaxeerX.MarketEvent do
  @moduledoc """
  An action emitted by one of the LayerX market precompiles.

  One row per log, keyed by the emitting transaction hash and the log index. `domain`
  says which precompile emitted it — exchange, bridge or launchpad — and `kind` keeps
  the event name that domain uses, so a precompile upgrade that adds an event does not
  need a migration.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.MarketEvents`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @domains ~w(exchange bridge launchpad)a

  @required_attrs ~w(domain kind block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters address_hash account asset_id asset_address_hash amount block_consensus)a

  @typedoc """
  * `domain` - the market precompile that emitted the action.
  * `kind` - the name of the event within that domain.
  * `event_name` - the Solidity name of the precompile event the row was decoded from.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `address_hash` - the EVM address acting in the event.
  * `account` - the kernel account the event belongs to.
  * `asset_id` - the custody asset id the event moves, when the event carries one.
  * `asset_address_hash` - the EVM spelling of that asset, when the event carries one.
  * `amount` - the moved amount, when the event carries one.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_market_events" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:domain, Ecto.Enum, values: @domains, null: false)
    field(:kind, :string, null: false)
    field(:event_name, :string)
    field(:parameters, :map)
    field(:address_hash, Hash.Address)
    field(:account, Hash.Full)
    field(:asset_id, Hash.Full)
    field(:asset_address_hash, Hash.Address)
    field(:amount, :decimal)

    belongs_to(:transaction, Transaction,
      foreign_key: :transaction_hash,
      primary_key: true,
      references: :hash,
      type: Hash.Full,
      null: false
    )

    belongs_to(:block, Block,
      foreign_key: :block_hash,
      references: :hash,
      type: Hash.Full,
      null: false
    )

    timestamps()
  end

  @doc """
  The market domains the schema admits.
  """
  @spec domains() :: [atom()]
  def domains, do: @domains

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = event, attrs) do
    event
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> validate_length(:kind, min: 1, max: 255)
    |> validate_number(:amount, greater_than_or_equal_to: 0)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_market_events_pkey)
  end

  @doc """
  Query over the market events that belong to a consensus block.

  The consensus flag is read through the block association so that the result is
  correct even before `block_consensus` has been denormalized onto the row.
  """
  @spec only_consensus_query() :: Ecto.Query.t()
  def only_consensus_query do
    from(event in __MODULE__,
      inner_join: block in assoc(event, :block),
      as: :block,
      where: block.consensus == true
    )
  end

  @doc """
  Query selecting the market events of the given block hashes, for denormalizing the
  loss of consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(event in __MODULE__, where: event.block_hash in ^block_hashes)
  end
end
