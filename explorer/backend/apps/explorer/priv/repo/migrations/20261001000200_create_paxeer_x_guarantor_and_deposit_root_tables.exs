defmodule Explorer.Repo.Migrations.CreatePaxeerXGuarantorAndDepositRootTables do
  use Ecto.Migration

  def change do
    execute(
      "CREATE TYPE lx_guarantor_event_kind AS ENUM ('guarantor_registered', 'guarantor_activated', 'guarantor_slashed', 'bond_increased', 'unbond_begun', 'unbond_completed', 'challenge_opened', 'challenge_resolved', 'availability_attested', 'sequencer_authorized')",
      "DROP TYPE lx_guarantor_event_kind"
    )

    create table(:lx_guarantor_events, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)

      add(:kind, :lx_guarantor_event_kind, null: false)
      add(:guarantor_id, :bytea, null: true)
      add(:signer_address_hash, :bytea, null: true)
      add(:operator_address_hash, :bytea, null: true)
      # 10^x = 2^256, x ~ 77.064, so 78 decimal digits hold the full 256-bit EVM amount
      add(:bond, :numeric, precision: 78, scale: 0, null: true)
      add(:amount, :numeric, precision: 78, scale: 0, null: true)
      add(:batch_number, :bigint, null: true)
      add(:challenge_id, :bigint, null: true)
      add(:challenge_kind, :integer, null: true)
      add(:upheld, :boolean, null: true)
      add(:evidence_hash, :bytea, null: true)
      add(:challenger_address_hash, :bytea, null: true)
      add(:reporter_address_hash, :bytea, null: true)
      add(:reporter_reward, :numeric, precision: 78, scale: 0, null: true)
      add(:class_mask, :integer, null: true)
      add(:availability_mask, :integer, null: true)
      add(:sequencer_id, :bytea, null: true)
      add(:sequencer_public_key, :bytea, null: true)
      add(:first_batch_number, :bigint, null: true)
      add(:last_batch_number, :bigint, null: true)
      add(:completion_time, :bigint, null: true)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_guarantor_events, [:block_number]))
    create(index(:lx_guarantor_events, [:block_hash]))
    create(index(:lx_guarantor_events, [:block_consensus]))
    create(index(:lx_guarantor_events, [:kind]))
    create(index(:lx_guarantor_events, [:guarantor_id]))
    create(index(:lx_guarantor_events, [:batch_number]))
    create(index(:lx_guarantor_events, [:challenge_id]))
    create(index(:lx_guarantor_events, [:sequencer_id]))

    create table(:lx_deposit_roots, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)

      add(:checkpoint_id, :bytea, null: false)
      add(:deposit_root, :bytea, null: false)
      add(:commitment, :bytea, null: false)
      add(:version, :integer, null: false)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_deposit_roots, [:block_number]))
    create(index(:lx_deposit_roots, [:block_hash]))
    create(index(:lx_deposit_roots, [:block_consensus]))
    create(index(:lx_deposit_roots, [:checkpoint_id]))
    create(index(:lx_deposit_roots, [:deposit_root]))
  end
end
