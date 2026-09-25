defmodule Explorer.Chain.PaxeerX.DepositRoot do
  @moduledoc """
  A deposit root the LayerX custody precompile registered against a checkpoint.

  One row per log, keyed by the emitting transaction hash, the block hash and the log
  index the way an upstream log is keyed, so a transaction a reorg moves into another
  block keeps a row per block rather than losing the later one. The registration is not
  a custody movement — it moves no balance and belongs to no
  deposit or claim flow — so it is held here rather than folded into
  `Explorer.Chain.PaxeerX.CustodyEvent` and its kind enumeration.

  `checkpoint_id` is the checkpoint the root is registered against, `deposit_root` the
  root of the deposits that checkpoint admits, `commitment` the commitment published
  beside it and `version` the version of the commitment scheme, all four of which every
  registration carries.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.DepositRoots`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  # The precompile emits the commitment scheme version as a uint16.
  @uint16_maximum 65_535

  @required_attrs ~w(checkpoint_id deposit_root commitment version block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters block_consensus)a

  @typedoc """
  * `checkpoint_id` - the checkpoint the deposit root is registered against.
  * `deposit_root` - the root of the deposits that checkpoint admits.
  * `commitment` - the commitment published beside the root.
  * `version` - the version of the commitment scheme.
  * `event_name` - the Solidity name of the precompile event the row was decoded from.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_deposit_roots" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:checkpoint_id, Hash.Full, null: false)
    field(:deposit_root, Hash.Full, null: false)
    field(:commitment, Hash.Full, null: false)
    field(:version, :integer, null: false)
    field(:event_name, :string)
    field(:parameters, :map)

    belongs_to(:transaction, Transaction,
      foreign_key: :transaction_hash,
      primary_key: true,
      references: :hash,
      type: Hash.Full,
      null: false
    )

    belongs_to(:block, Block,
      foreign_key: :block_hash,
      primary_key: true,
      references: :hash,
      type: Hash.Full,
      null: false
    )

    timestamps()
  end

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = deposit_root, attrs) do
    deposit_root
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> validate_number(:version, greater_than_or_equal_to: 0, less_than_or_equal_to: @uint16_maximum)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :block_hash, :log_index], name: :lx_deposit_roots_pkey)
  end

  @doc """
  Query over the deposit roots that belong to a consensus block.

  The consensus flag is read through the block association so that the result is
  correct even before `block_consensus` has been denormalized onto the row.
  """
  @spec only_consensus_query() :: Ecto.Query.t()
  def only_consensus_query do
    from(deposit_root in __MODULE__,
      inner_join: block in assoc(deposit_root, :block),
      as: :block,
      where: block.consensus == true
    )
  end

  @doc """
  Query selecting the deposit roots of the given block hashes, for denormalizing the
  loss of consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(deposit_root in __MODULE__, where: deposit_root.block_hash in ^block_hashes)
  end
end
