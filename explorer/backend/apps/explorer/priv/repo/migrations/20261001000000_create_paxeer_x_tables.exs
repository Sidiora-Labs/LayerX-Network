defmodule Explorer.Repo.Migrations.CreatePaxeerXTables do
  use Ecto.Migration

  def change do
    execute(
      "CREATE TYPE lx_custody_event_kind AS ENUM ('custody_deposit', 'claim_queued', 'claim_finalised', 'custody_release', 'emergency_exit')",
      "DROP TYPE lx_custody_event_kind"
    )

    execute(
      "CREATE TYPE lx_custody_event_direction AS ENUM ('deposit', 'withdrawal', 'balance_delta')",
      "DROP TYPE lx_custody_event_direction"
    )

    execute(
      "CREATE TYPE lx_anchor_status AS ENUM ('unknown', 'submitted', 'final')",
      "DROP TYPE lx_anchor_status"
    )

    execute(
      "CREATE TYPE lx_receipt_status AS ENUM ('unverified', 'sequencer_signed', 'batch_included', 'state_proven', 'checkpoint_finalised', 'settlement_anchored')",
      "DROP TYPE lx_receipt_status"
    )

    execute(
      "CREATE TYPE lx_market_domain AS ENUM ('exchange', 'bridge', 'launchpad')",
      "DROP TYPE lx_market_domain"
    )

    create table(:lx_account_bindings, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false
      )

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)

      add(:evm_address_hash, :bytea, null: false)
      add(:pax_address, :string, null: true)
      add(:layerx_did, :string, null: true)
      add(:layerx_account, :string, null: true)
      add(:bound, :boolean, null: false)
      add(:nonce, :bigint, null: true)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_account_bindings, [:block_number]))
    create(index(:lx_account_bindings, [:block_hash]))
    create(index(:lx_account_bindings, [:block_consensus]))

    create(
      index(:lx_account_bindings, [:evm_address_hash, :block_number, :log_index],
        name: :lx_account_bindings_evm_address_hash_position_index
      )
    )

    create(index(:lx_account_bindings, [:layerx_did]))
    create(index(:lx_account_bindings, [:layerx_account]))
    create(index(:lx_account_bindings, [:pax_address]))

    create table(:lx_custody_events, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false
      )

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)

      add(:kind, :lx_custody_event_kind, null: false)
      add(:direction, :lx_custody_event_direction, null: false)
      add(:reference_id, :bytea, null: true)
      add(:asset_id, :bytea, null: true)
      # 10^x = 2^256, x ~ 77.064, so 78 decimal digits hold the full 256-bit EVM amount
      add(:amount, :numeric, precision: 78, scale: 0, null: true)
      add(:address_hash, :bytea, null: true)
      add(:account, :bytea, null: true)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_custody_events, [:block_number]))
    create(index(:lx_custody_events, [:block_hash]))
    create(index(:lx_custody_events, [:block_consensus]))
    create(index(:lx_custody_events, [:address_hash, :block_number, :log_index]))
    create(index(:lx_custody_events, [:account, :block_number, :log_index]))
    create(index(:lx_custody_events, [:asset_id]))
    create(index(:lx_custody_events, [:reference_id]))
    create(index(:lx_custody_events, [:kind]))

    create table(:lx_anchors, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false
      )

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)

      add(:batch_number, :bigint, null: false)
      add(:checkpoint_id, :bytea, null: false)
      add(:state_root, :bytea, null: true)
      add(:receipt_root, :bytea, null: true)
      add(:signers, :integer, null: true)
      add(:status, :lx_anchor_status, null: false)
      add(:kernel_height, :bigint, null: true)
      add(:sealed_height, :bigint, null: true)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_anchors, [:block_number]))
    create(index(:lx_anchors, [:block_hash]))
    create(index(:lx_anchors, [:block_consensus]))
    create(index(:lx_anchors, [:batch_number]))
    create(index(:lx_anchors, [:checkpoint_id]))
    create(index(:lx_anchors, [:status, :batch_number]))

    create table(:lx_receipts, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false
      )

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)

      add(:receipt_id, :bytea, null: false)
      add(:account, :bytea, null: true)
      add(:payload_hash, :bytea, null: true)
      add(:status, :lx_receipt_status, null: false)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_receipts, [:block_number]))
    create(index(:lx_receipts, [:block_hash]))
    create(index(:lx_receipts, [:block_consensus]))
    create(index(:lx_receipts, [:receipt_id]))
    create(index(:lx_receipts, [:account, :block_number, :log_index]))
    create(index(:lx_receipts, [:status]))

    create table(:lx_market_events, primary_key: false) do
      add(
        :transaction_hash,
        references(:transactions, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false,
        primary_key: true
      )

      add(:log_index, :integer, null: false, primary_key: true)

      add(
        :block_hash,
        references(:blocks, column: :hash, on_delete: :delete_all, type: :bytea),
        null: false
      )

      add(:block_number, :bigint, null: false)
      add(:block_consensus, :boolean, null: false, default: true)

      add(:domain, :lx_market_domain, null: false)
      add(:kind, :string, null: false)
      add(:address_hash, :bytea, null: true)
      add(:account, :bytea, null: true)
      add(:asset_id, :bytea, null: true)
      add(:asset_address_hash, :bytea, null: true)
      # 10^x = 2^256, x ~ 77.064, so 78 decimal digits hold the full 256-bit EVM amount
      add(:amount, :numeric, precision: 78, scale: 0, null: true)

      timestamps(null: false, type: :utc_datetime_usec)
    end

    create(index(:lx_market_events, [:block_number]))
    create(index(:lx_market_events, [:block_hash]))
    create(index(:lx_market_events, [:block_consensus]))
    create(index(:lx_market_events, [:domain, :block_number, :log_index]))
    create(index(:lx_market_events, [:address_hash, :block_number, :log_index]))
    create(index(:lx_market_events, [:account, :block_number, :log_index]))
    create(index(:lx_market_events, [:asset_id]))
    create(index(:lx_market_events, [:asset_address_hash]))
  end
end
