defmodule Explorer.Chain.PaxeerX.ReceiptTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.Receipts
  alias Explorer.Chain.PaxeerX.Receipt

  describe "changeset/2" do
    test "accepts a receipt with an account and a payload hash" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset = Receipt.changeset(%Receipt{}, attributes(block, transaction))

      assert changeset.valid?
      assert {:ok, receipt} = Repo.insert(changeset)
      assert receipt.status == :batch_included
      refute is_nil(receipt.receipt_id)
      refute is_nil(receipt.account)
      refute is_nil(receipt.payload_hash)
      assert receipt.block_number == block.number
    end

    test "requires the key, the receipt id and the status" do
      changeset = Receipt.changeset(%Receipt{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:receipt_id] == ["can't be blank"]
      assert errors[:status] == ["can't be blank"]
    end

    test "admits every rung of the verification lattice" do
      %{block: block, transaction: transaction} = block_with_transaction()

      assert Receipt.statuses() == [
               :unverified,
               :sequencer_signed,
               :batch_included,
               :state_proven,
               :checkpoint_finalised,
               :settlement_anchored
             ]

      for {status, index} <- Enum.with_index(Receipt.statuses()) do
        attrs =
          block
          |> attributes(transaction)
          |> Map.merge(%{status: status, log_index: index, receipt_id: block_hash()})

        assert {:ok, receipt} = %Receipt{} |> Receipt.changeset(attrs) |> Repo.insert()
        assert receipt.status == status
      end
    end

    test "rejects a rung outside the verification lattice" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:status, :accepted)

      changeset = Receipt.changeset(%Receipt{}, attrs)

      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:status]
    end
  end

  describe "run/3" do
    test "inserts a receipt row keyed by transaction hash and log index" do
      %{block: block, transaction: transaction} = block_with_transaction()

      assert {:ok, %{insert_paxeer_x_receipts: [inserted]}} = run_changes([attributes(block, transaction)])

      assert inserted.transaction_hash == transaction.hash
      assert inserted.log_index == 0
      assert inserted.status == :batch_included
      assert inserted.block_consensus
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_receipts: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_receipts: []}} =
               run_changes([Map.put(attrs, :status, :settlement_anchored)])

      assert [stored] = Repo.all(Receipt)
      assert stored.status == :batch_included
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_receipts: []}} = run_changes([])
    end
  end

  describe "only_consensus_query/0" do
    test "drops the rows of a block that lost consensus" do
      %{block: kept_block, transaction: kept_transaction} = block_with_transaction()
      %{block: reorged_block, transaction: reorged_transaction} = block_with_transaction()

      assert {:ok, _} =
               run_changes([
                 attributes(kept_block, kept_transaction),
                 attributes(reorged_block, reorged_transaction)
               ])

      Repo.update!(Ecto.Changeset.change(reorged_block, consensus: false))

      assert [kept] = Repo.all(Receipt.only_consensus_query())
      assert kept.transaction_hash == kept_transaction.hash

      assert {1, nil} =
               [reorged_block.hash]
               |> Receipt.lose_consensus_query()
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
      log_index: 0,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      receipt_id: block_hash(),
      account: block_hash(),
      payload_hash: block_hash(),
      status: :batch_included
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> Receipts.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
