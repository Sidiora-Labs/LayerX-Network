defmodule Explorer.Chain.PaxeerX.Anchor do
  @moduledoc """
  A checkpoint the LayerX anchor precompile submitted or finalized.

  One row per log, keyed by the emitting transaction hash and the log index, so the
  submission and the finalization of the same checkpoint are two rows that share a
  `batch_number` and a `checkpoint_id`. `status` follows the anchor ladder the network
  reports: unknown, submitted, final.

  `kernel_height` is the kernel height the checkpoint commits to and `sealed_height` the
  height the checkpoint seals, both recorded per checkpoint rather than read from a
  process-wide constant.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.Anchors`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @statuses ~w(unknown submitted final)a

  @required_attrs ~w(batch_number checkpoint_id status block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters state_root receipt_root signers kernel_height sealed_height block_consensus)a

  @typedoc """
  * `batch_number` - the checkpoint height, that is the anchor batch number.
  * `checkpoint_id` - the identifier of the checkpoint.
  * `event_name` - the Solidity name of the precompile event the row was decoded from.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `state_root` - the state root the checkpoint commits to.
  * `receipt_root` - the receipt root the checkpoint commits to.
  * `signers` - the number of guarantors that signed the checkpoint.
  * `status` - the rung the checkpoint has reached on the anchor ladder.
  * `kernel_height` - the kernel height the checkpoint commits to.
  * `sealed_height` - the height the checkpoint seals.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_anchors" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:batch_number, :integer, null: false)
    field(:checkpoint_id, Hash.Full, null: false)
    field(:event_name, :string)
    field(:parameters, :map)
    field(:state_root, Hash.Full)
    field(:receipt_root, Hash.Full)
    field(:signers, :integer)
    field(:status, Ecto.Enum, values: @statuses, null: false)
    field(:kernel_height, :integer)
    field(:sealed_height, :integer)

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
  The rungs of the anchor ladder the schema admits.
  """
  @spec statuses() :: [atom()]
  def statuses, do: @statuses

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = anchor, attrs) do
    anchor
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> validate_number(:batch_number, greater_than_or_equal_to: 0)
    |> validate_number(:signers, greater_than_or_equal_to: 0)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_anchors_pkey)
  end

  @doc """
  Query over the anchors that belong to a consensus block.

  The consensus flag is read through the block association so that the result is
  correct even before `block_consensus` has been denormalized onto the row.
  """
  @spec only_consensus_query() :: Ecto.Query.t()
  def only_consensus_query do
    from(anchor in __MODULE__,
      inner_join: block in assoc(anchor, :block),
      as: :block,
      where: block.consensus == true
    )
  end

  @doc """
  Query selecting the anchors of the given block hashes, for denormalizing the loss of
  consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(anchor in __MODULE__, where: anchor.block_hash in ^block_hashes)
  end

  @doc """
  Query for the highest finalized checkpoint recorded by a consensus block.
  """
  @spec latest_finalized_query() :: Ecto.Query.t()
  def latest_finalized_query do
    from(anchor in only_consensus_query(),
      where: anchor.status == :final,
      order_by: [desc: anchor.batch_number, desc: anchor.block_number, desc: anchor.log_index],
      limit: 1
    )
  end
end
