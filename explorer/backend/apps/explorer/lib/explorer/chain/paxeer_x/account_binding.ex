defmodule Explorer.Chain.PaxeerX.AccountBinding do
  @moduledoc """
  A binding between an EVM address and its LayerX kernel identity, as emitted by the
  `addr` precompile.

  One row per `LayerXBound` / `LayerXUnbound` log, keyed by the emitting transaction
  hash and the log index. The identity columns carry the four spellings of the unified
  account: the EVM address, the `pax` bech32 address, the `did:layerx:` DID and the
  kernel account id.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.AccountBindings`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @required_attrs ~w(evm_address_hash bound block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters pax_address layerx_did layerx_account nonce block_consensus)a

  @typedoc """
  * `evm_address_hash` - the EVM address the binding is about.
  * `event_name` - the Solidity name of the precompile event the row was decoded from.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `pax_address` - the `pax` bech32 spelling of the same account, when known.
  * `layerx_did` - the `did:layerx:<64 hex>` decentralized identifier, when known.
  * `layerx_account` - the kernel account id derived from the DID, when known.
  * `bound` - `true` for a binding log, `false` for an unbinding log.
  * `nonce` - the bind nonce carried by the log.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_account_bindings" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:evm_address_hash, Hash.Address, null: false)
    field(:event_name, :string)
    field(:parameters, :map)
    field(:pax_address, :string)
    field(:layerx_did, :string)
    field(:layerx_account, :string)
    field(:bound, :boolean, null: false)
    field(:nonce, :integer)

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

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = binding, attrs) do
    binding
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_account_bindings_pkey)
  end

  @doc """
  Query over the bindings that belong to a consensus block.

  The consensus flag is read through the block association so that the result is
  correct even before `block_consensus` has been denormalized onto the row.
  """
  @spec only_consensus_query() :: Ecto.Query.t()
  def only_consensus_query do
    from(binding in __MODULE__,
      inner_join: block in assoc(binding, :block),
      as: :block,
      where: block.consensus == true
    )
  end

  @doc """
  Query selecting the bindings of the given block hashes, for denormalizing the loss of
  consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(binding in __MODULE__, where: binding.block_hash in ^block_hashes)
  end
end
