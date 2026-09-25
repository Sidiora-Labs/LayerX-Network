defmodule Explorer.Chain.PaxeerX.GuarantorEvent do
  @moduledoc """
  A guarantor, challenge, availability or sequencer-authorisation event of the LayerX
  anchor precompile.

  One row per log, keyed by the emitting transaction hash and the log index, so the
  registration, the bond increases, the slashes and the unbonding of one guarantor are
  separate rows that share a `guarantor_id`. `kind` names the precompile event the row
  was decoded from, so a reader can select the flow it cares about without parsing
  `event_name`.

  The event columns are the union of the arguments those events carry, and a column is
  populated only by the events that supply it: `bond` is the guarantor's bond after the
  event, `amount` the value the event moves — the bond increase, the slashed value or
  the unbonded value — and `completion_time` the moment an unbonding becomes claimable.
  `parameters` keeps every decoded argument under its ABI name, so an argument without a
  column of its own, such as the registration status or the slash reason, is still
  readable.

  Rows inherit the consensus of the block that produced them: `block_consensus` is the
  denormalized copy of `blocks.consensus`, and `only_consensus_query/0` reads the
  authoritative flag through the block association, so a reorged block's rows drop out
  of every read the same way token transfers do.

  Changes in the schema should be reflected in the bulk import module:
  - `Explorer.Chain.Import.Runner.PaxeerX.GuarantorEvents`
  """
  use Explorer.Schema

  alias Explorer.Chain.{Block, Hash, Transaction}

  @kinds ~w(guarantor_registered guarantor_activated guarantor_slashed bond_increased unbond_begun unbond_completed challenge_opened challenge_resolved availability_attested sequencer_authorized)a

  # The precompile emits every mask and the challenge kind as a uint8.
  @uint8_maximum 255

  @required_attrs ~w(kind block_hash block_number transaction_hash log_index)a
  @optional_attrs ~w(event_name parameters guarantor_id signer_address_hash operator_address_hash bond amount batch_number challenge_id challenge_kind upheld evidence_hash challenger_address_hash reporter_address_hash reporter_reward class_mask availability_mask sequencer_id sequencer_public_key first_batch_number last_batch_number completion_time block_consensus)a

  @typedoc """
  * `kind` - the precompile event the row was decoded from.
  * `event_name` - the Solidity name of that event.
  * `parameters` - every decoded argument of that event under its ABI name, lossless.
  * `guarantor_id` - the identifier of the guarantor the event concerns.
  * `signer_address_hash` - the EVM address the guarantor signs checkpoints with.
  * `operator_address_hash` - the EVM address that operates the guarantor.
  * `bond` - the guarantor's bond after the event.
  * `amount` - the value the event moves: a bond increase, a slash or an unbonding.
  * `batch_number` - the anchor batch the event concerns.
  * `challenge_id` - the identifier of the challenge the event concerns.
  * `challenge_kind` - the challenge kind as the precompile enumerates it.
  * `upheld` - whether a resolved challenge was upheld.
  * `evidence_hash` - the hash of the evidence an opened challenge carries.
  * `challenger_address_hash` - the EVM address that opened the challenge.
  * `reporter_address_hash` - the EVM address that reported the slashed behaviour.
  * `reporter_reward` - the share of the slash paid to that reporter.
  * `class_mask` - the data-availability classes an attestation covers.
  * `availability_mask` - the classes the attester reports available.
  * `sequencer_id` - the identifier of the authorised sequencer.
  * `sequencer_public_key` - the public key that authorisation binds.
  * `first_batch_number` - the first batch the authorisation covers.
  * `last_batch_number` - the last batch the authorisation covers.
  * `completion_time` - the moment a begun unbonding becomes claimable.
  * `transaction_hash` - the hash of the transaction that emitted the log.
  * `log_index` - the index of the log within its block.
  * `block_hash` - the hash of the block that holds the transaction.
  * `block_number` - the number of that block.
  * `block_consensus` - denormalized copy of the block's consensus flag.
  """
  @primary_key false
  typed_schema "lx_guarantor_events" do
    field(:log_index, :integer, primary_key: true, null: false)
    field(:block_number, :integer, null: false) :: Block.block_number()
    field(:block_consensus, :boolean, null: false)
    field(:kind, Ecto.Enum, values: @kinds, null: false)
    field(:event_name, :string)
    field(:parameters, :map)
    field(:guarantor_id, Hash.Full)
    field(:signer_address_hash, Hash.Address)
    field(:operator_address_hash, Hash.Address)
    field(:bond, :decimal)
    field(:amount, :decimal)
    field(:batch_number, :integer)
    field(:challenge_id, :integer)
    field(:challenge_kind, :integer)
    field(:upheld, :boolean)
    field(:evidence_hash, Hash.Full)
    field(:challenger_address_hash, Hash.Address)
    field(:reporter_address_hash, Hash.Address)
    field(:reporter_reward, :decimal)
    field(:class_mask, :integer)
    field(:availability_mask, :integer)
    field(:sequencer_id, Hash.Full)
    field(:sequencer_public_key, Hash.Full)
    field(:first_batch_number, :integer)
    field(:last_batch_number, :integer)
    field(:completion_time, :integer)

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
  The precompile events the schema admits.
  """
  @spec kinds() :: [atom()]
  def kinds, do: @kinds

  @spec changeset(Ecto.Schema.t(), map()) :: Ecto.Changeset.t()
  def changeset(%__MODULE__{} = event, attrs) do
    event
    |> cast(attrs, @required_attrs ++ @optional_attrs)
    |> validate_required(@required_attrs)
    |> validate_number(:bond, greater_than_or_equal_to: 0)
    |> validate_number(:amount, greater_than_or_equal_to: 0)
    |> validate_number(:reporter_reward, greater_than_or_equal_to: 0)
    |> validate_number(:batch_number, greater_than_or_equal_to: 0)
    |> validate_number(:challenge_id, greater_than_or_equal_to: 0)
    |> validate_number(:first_batch_number, greater_than_or_equal_to: 0)
    |> validate_number(:last_batch_number, greater_than_or_equal_to: 0)
    |> validate_number(:completion_time, greater_than_or_equal_to: 0)
    |> validate_number(:challenge_kind, greater_than_or_equal_to: 0, less_than_or_equal_to: @uint8_maximum)
    |> validate_number(:class_mask, greater_than_or_equal_to: 0, less_than_or_equal_to: @uint8_maximum)
    |> validate_number(:availability_mask, greater_than_or_equal_to: 0, less_than_or_equal_to: @uint8_maximum)
    |> foreign_key_constraint(:transaction_hash)
    |> foreign_key_constraint(:block_hash)
    |> unique_constraint([:transaction_hash, :log_index], name: :lx_guarantor_events_pkey)
  end

  @doc """
  Query over the guarantor events that belong to a consensus block.

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
  Query selecting the guarantor events of the given block hashes, for denormalizing the
  loss of consensus onto `block_consensus`.
  """
  @spec lose_consensus_query([Hash.Full.t()]) :: Ecto.Query.t()
  def lose_consensus_query(block_hashes) when is_list(block_hashes) do
    from(event in __MODULE__, where: event.block_hash in ^block_hashes)
  end
end
