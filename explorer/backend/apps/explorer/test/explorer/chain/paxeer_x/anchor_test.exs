defmodule Explorer.Chain.PaxeerX.AnchorTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.Anchors
  alias Explorer.Chain.PaxeerX.Anchor

  describe "changeset/2" do
    test "accepts a submitted checkpoint with both roots" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset = Anchor.changeset(%Anchor{}, attributes(block, transaction))

      assert changeset.valid?
      assert {:ok, anchor} = Repo.insert(changeset)
      assert anchor.status == :submitted
      assert anchor.batch_number == 4211
      assert anchor.signers == 5
      assert anchor.kernel_height == 918_244
      assert anchor.sealed_height == 918_100
      refute is_nil(anchor.state_root)
      refute is_nil(anchor.receipt_root)
    end

    test "requires the key, the batch number, the checkpoint id and the status" do
      changeset = Anchor.changeset(%Anchor{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:batch_number] == ["can't be blank"]
      assert errors[:checkpoint_id] == ["can't be blank"]
      assert errors[:status] == ["can't be blank"]
    end

    test "rejects a rung outside the anchor ladder" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:status, :settled)

      changeset = Anchor.changeset(%Anchor{}, attrs)

      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:status]
      assert Anchor.statuses() == [:unknown, :submitted, :final]
    end

    test "rejects a negative batch number" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:batch_number, -1)

      changeset = Anchor.changeset(%Anchor{}, attrs)

      refute changeset.valid?
      assert changeset_errors(changeset)[:batch_number] == ["must be greater than or equal to 0"]
    end
  end

  describe "run/3" do
    test "keeps the submission and the finalisation of one checkpoint as two rows" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changes = [
        attrs,
        Map.merge(attrs, %{log_index: 2, status: :final, signers: nil})
      ]

      assert {:ok, %{insert_paxeer_x_anchors: inserted}} = run_changes(changes)

      assert length(inserted) == 2

      assert [:submitted, :final] ==
               Anchor
               |> order_by([anchor], asc: anchor.log_index)
               |> select([anchor], anchor.status)
               |> Repo.all()

      assert [4211, 4211] ==
               Anchor
               |> order_by([anchor], asc: anchor.log_index)
               |> select([anchor], anchor.batch_number)
               |> Repo.all()
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_anchors: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_anchors: []}} =
               run_changes([Map.merge(attrs, %{status: :final, signers: 9})])

      assert [stored] = Repo.all(Anchor)
      assert stored.status == :submitted
      assert stored.signers == 5
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_anchors: []}} = run_changes([])
    end
  end

  describe "latest_finalized_query/0" do
    test "returns the highest finalized checkpoint of a consensus block" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, _} =
               run_changes([
                 Map.merge(attrs, %{log_index: 1, batch_number: 4210, status: :final}),
                 Map.merge(attrs, %{log_index: 2, batch_number: 4211, status: :final}),
                 Map.merge(attrs, %{log_index: 3, batch_number: 4212, status: :submitted})
               ])

      assert %Anchor{batch_number: 4211, status: :final} = Repo.one(Anchor.latest_finalized_query())
    end

    test "ignores the checkpoints of a block that lost consensus" do
      %{block: kept_block, transaction: kept_transaction} = block_with_transaction()
      %{block: reorged_block, transaction: reorged_transaction} = block_with_transaction()

      kept = kept_block |> attributes(kept_transaction) |> Map.merge(%{batch_number: 4210, status: :final})

      reorged =
        reorged_block |> attributes(reorged_transaction) |> Map.merge(%{batch_number: 4299, status: :final})

      assert {:ok, _} = run_changes([kept, reorged])

      Repo.update!(Ecto.Changeset.change(reorged_block, consensus: false))

      assert %Anchor{batch_number: 4210} = Repo.one(Anchor.latest_finalized_query())
      assert Repo.aggregate(Anchor.only_consensus_query(), :count) == 1

      assert {1, nil} =
               [reorged_block.hash]
               |> Anchor.lose_consensus_query()
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
      batch_number: 4211,
      checkpoint_id: block_hash(),
      state_root: block_hash(),
      receipt_root: block_hash(),
      signers: 5,
      status: :submitted,
      kernel_height: 918_244,
      sealed_height: 918_100
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> Anchors.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
