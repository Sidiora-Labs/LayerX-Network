defmodule Explorer.Repo.Migrations.AddPaxeerXEventProvenance do
  use Ecto.Migration

  def change do
    alter table(:lx_account_bindings) do
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)
    end

    alter table(:lx_custody_events) do
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)
      add(:nullifier, :bytea, null: true)
      add(:checkpoint_hash, :bytea, null: true)
    end

    alter table(:lx_anchors) do
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)
    end

    alter table(:lx_market_events) do
      add(:event_name, :string, null: true)
      add(:parameters, :jsonb, null: true)
    end

    create(index(:lx_custody_events, [:nullifier]))
    create(index(:lx_custody_events, [:checkpoint_hash]))
    create(index(:lx_market_events, [:kind]))
  end
end
