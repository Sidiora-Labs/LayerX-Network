defmodule Explorer.Chain.PaxeerX.CustodyEvent do
  @moduledoc """
  A custody movement emitted by the LayerX custody precompile.

  One row per log, keyed by the emitting transaction hash and the log index. `kind`
  keeps the event as the custody flow names it — deposit, claim queued, claim finalised,
  release, emergency exit — and `direction` folds that flow into the three balance
  effects a reader cares about: a deposit into custody, a withdrawal out of it, or a
  balance delta that is neither.

  `reference_id` carries the deposit id or the claim id that ties the rows of one flow
  together; `address_hash` is the EVM actor and `account` the kernel account, matching
  the `address` / `account` pair of a unified activity item.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.CustodyEvents`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @kinds ~w(custody_deposit claim_queued claim_finalised custody_release emergency_exit)a
  @directions ~w(deposit withdrawal balance_delta)a

  @required_attrs ~w(kind direction block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters nullifier checkpoint_hash reference_id asset_id amount address_hash account block_consensus)a

  @typedoc """
  * `kind` - the custody event as the precompile emits it.
  * `direction` - the balance effect of the event on the custodied account.
  * `event_name` - the Solidity name of the precompile event the row was decoded from.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `reference_id` - the deposit id or claim id shared by the rows of one custody flow.
  * `nullifier` - the claim nullifier that links a queued claim to its finalisation or exit.
  * `checkpoint_hash` - the checkpoint the claim proves against, as the custody ABI spells it.
  * `asset_id` - the custody asset id the event moves, when the event carries one.
  * `amount` - the moved amount, when the event carries one.
  * `address_hash` - the EVM address acting in the event (payer or recipient).
  * `account` - the kernel account the event credits or debits.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_custody_events" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:kind, Ecto.Enum, values: @kinds, null: false)
    field(:direction, Ecto.Enum, values: @directions, null: false)
    field(:event_name, :string)
    field(:parameters, :map)
    field(:reference_id, Hash.Full)
    field(:nullifier, Hash.Full)
    field(:checkpoint_hash, Hash.Full)
    field(:asset_id, Hash.Full)
    field(:amount, :decimal)
    field(:address_hash, Hash.Address)
    field(:account, Hash.Full)

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
  The custody event kinds the schema admits.
  """
  @spec kinds() :: [atom()]
  def kinds, do: @kinds

  @doc """
  The balance effects the schema admits.
  """
  @spec directions() :: [atom()]
  def directions, do: @directions

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = event, attrs) do
    event
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> validate_number(:amount, greater_than_or_equal_to: 0)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_custody_events_pkey)
  end

  @doc """
  Query over the custody events that belong to a consensus block.

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
  Query selecting the custody events of the given block hashes, for denormalizing the
  loss of consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(event in __MODULE__, where: event.block_hash in ^block_hashes)
  end
end
