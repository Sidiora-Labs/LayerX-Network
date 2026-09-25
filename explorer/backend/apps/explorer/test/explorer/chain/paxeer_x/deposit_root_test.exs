defmodule Explorer.Chain.PaxeerX.DepositRootTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.DepositRoots
  alias Explorer.Chain.PaxeerX.{CustodyEvent, DepositRoot}

  describe "changeset/2" do
    test "accepts a registration with its checkpoint, root, commitment and version" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changeset = DepositRoot.changeset(%DepositRoot{}, attrs)

      assert changeset.valid?
      assert {:ok, deposit_root} = Repo.insert(changeset)
      assert deposit_root.checkpoint_id == attrs.checkpoint_id
      assert deposit_root.deposit_root == attrs.deposit_root
      assert deposit_root.commitment == attrs.commitment
      assert deposit_root.version == 1
      assert deposit_root.event_name == "DepositRootRegistered"
      assert deposit_root.parameters["version"] == 1
      assert deposit_root.block_number == block.number
      assert deposit_root.block_consensus == true
    end

    test "requires the key, the block, the checkpoint, the root, the commitment and the version" do
      changeset = DepositRoot.changeset(%DepositRoot{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:checkpoint_id] == ["can't be blank"]
      assert errors[:deposit_root] == ["can't be blank"]
      assert errors[:commitment] == ["can't be blank"]
      assert errors[:version] == ["can't be blank"]
    end

    test "rejects a version wider than the precompile emits" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset =
        DepositRoot.changeset(%DepositRoot{}, block |> attributes(transaction) |> Map.put(:version, 65_536))

      refute changeset.valid?
      assert changeset_errors(changeset)[:version] == ["must be less than or equal to 65535"]
    end

    test "is held apart from the custody movement kinds rather than widening them" do
      assert CustodyEvent.kinds() == [
               :custody_deposit,
               :claim_queued,
               :claim_finalised,
               :custody_release,
               :emergency_exit
             ]

      %{rows: rows} =
        Repo.query!(
          "SELECT enumlabel FROM pg_enum JOIN pg_type ON pg_type.oid = pg_enum.enumtypid WHERE pg_type.typname = 'lx_custody_event_kind' ORDER BY enumsortorder"
        )

      assert List.flatten(rows) == ~w(custody_deposit claim_queued claim_finalised custody_release emergency_exit)
    end
  end

  describe "run/3" do
    test "keeps the registrations of two checkpoints as two rows" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)
      second_checkpoint = block_hash()

      changes = [
        attrs,
        Map.merge(attrs, %{log_index: 2, checkpoint_id: second_checkpoint, version: 2})
      ]

      assert {:ok, %{insert_paxeer_x_deposit_roots: inserted}} = run_changes(changes)
      assert length(inserted) == 2

      assert [first, second] = Repo.all(from(root in DepositRoot, order_by: [asc: root.log_index]))
      assert first.checkpoint_id == attrs.checkpoint_id
      assert first.version == 1
      assert second.checkpoint_id == second_checkpoint
      assert second.version == 2
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_deposit_roots: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_deposit_roots: []}} =
               run_changes([Map.merge(attrs, %{deposit_root: block_hash(), version: 9})])

      assert [stored] = Repo.all(DepositRoot)
      assert stored.deposit_root == attrs.deposit_root
      assert stored.version == 1
    end

    test "replaying a whole block keeps one row per log" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changes = [attrs, Map.merge(attrs, %{log_index: 2, checkpoint_id: block_hash()})]

      assert {:ok, %{insert_paxeer_x_deposit_roots: first}} = run_changes(changes)
      assert length(first) == 2

      assert {:ok, %{insert_paxeer_x_deposit_roots: []}} = run_changes(changes)

      assert Repo.aggregate(DepositRoot, :count) == 2
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_deposit_roots: []}} = run_changes([])
    end
  end

  describe "only_consensus_query/0 and lose_consensus_query/1" do
    test "drop the rows of a block that lost consensus" do
      %{block: kept_block, transaction: kept_transaction} = block_with_transaction()
      %{block: reorged_block, transaction: reorged_transaction} = block_with_transaction()

      assert {:ok, _} =
               run_changes([
                 attributes(kept_block, kept_transaction),
                 attributes(reorged_block, reorged_transaction)
               ])

      Repo.update!(Ecto.Changeset.change(reorged_block, consensus: false))

      assert Repo.aggregate(DepositRoot.only_consensus_query(), :count) == 1

      assert {1, nil} =
               [reorged_block.hash]
               |> DepositRoot.lose_consensus_query()
               |> Repo.update_all(set: [block_consensus: false])
    end
  end

  defp block_with_transaction do
    block = insert(:block)
    transaction = :transaction |> insert() |> with_block(block)

    %{block: block, transaction: transaction}
  end

  defp attributes(block, transaction) do
    %{
      transaction_hash: transaction.hash,
      log_index: 1,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      event_name: "DepositRootRegistered",
      parameters: %{"version" => 1},
      checkpoint_id: block_hash(),
      deposit_root: block_hash(),
      commitment: block_hash(),
      version: 1
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> DepositRoots.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
