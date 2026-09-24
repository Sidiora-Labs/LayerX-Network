defmodule Explorer.Chain.PaxeerX.Receipt do
  @moduledoc """
  A kernel receipt observed on the EVM side.

  One row per log, keyed by the emitting transaction hash and the log index. `status`
  is the receipt verification lattice the kernel publishes: unverified, sequencer
  signed, batch included, state proven, checkpoint finalised, settlement anchored.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.Receipts`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @statuses ~w(unverified sequencer_signed batch_included state_proven checkpoint_finalised settlement_anchored)a

  @required_attrs ~w(receipt_id status block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(account payload_hash block_consensus)a

  @typedoc """
  * `receipt_id` - the kernel identifier of the receipt.
  * `account` - the kernel account the receipt belongs to.
  * `payload_hash` - the hash of the receipt payload.
  * `status` - the rung the receipt has reached on the verification lattice.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_receipts" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:receipt_id, Hash.Full, null: false)
    field(:account, Hash.Full)
    field(:payload_hash, Hash.Full)
    field(:status, Ecto.Enum, values: @statuses, null: false)

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
  The rungs of the receipt verification lattice the schema admits.
  """
  @spec statuses() :: [atom()]
  def statuses, do: @statuses

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = receipt, attrs) do
    receipt
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_receipts_pkey)
  end

  @doc """
  Query over the receipts that belong to a consensus block.

  The consensus flag is read through the block association so that the result is
  correct even before `block_consensus` has been denormalized onto the row.
  """
  @spec only_consensus_query() :: Ecto.Query.t()
  def only_consensus_query do
    from(receipt in __MODULE__,
      inner_join: block in assoc(receipt, :block),
      as: :block,
      where: block.consensus == true
    )
  end

  @doc """
  Query selecting the receipts of the given block hashes, for denormalizing the loss of
  consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(receipt in __MODULE__, where: receipt.block_hash in ^block_hashes)
  end
end
