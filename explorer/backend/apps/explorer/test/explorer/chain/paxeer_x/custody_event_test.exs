defmodule Explorer.Chain.PaxeerX.CustodyEventTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.CustodyEvents
  alias Explorer.Chain.PaxeerX.CustodyEvent

  describe "changeset/2" do
    test "accepts a deposit carrying an asset and an amount" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset = CustodyEvent.changeset(%CustodyEvent{}, attributes(block, transaction))

      assert changeset.valid?
      assert {:ok, event} = Repo.insert(changeset)
      assert event.kind == :custody_deposit
      assert event.direction == :deposit
      assert Decimal.equal?(event.amount, Decimal.new(1_000_000))
      assert event.block_number == block.number
    end

    test "accepts a claim finalisation that carries neither asset nor amount" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs =
        block
        |> attributes(transaction)
        |> Map.merge(%{
          kind: :claim_finalised,
          direction: :balance_delta,
          asset_id: nil,
          amount: nil,
          address_hash: nil,
          account: nil
        })

      assert {:ok, event} = %CustodyEvent{} |> CustodyEvent.changeset(attrs) |> Repo.insert()
      assert event.kind == :claim_finalised
      assert is_nil(event.amount)
      refute is_nil(event.reference_id)
    end

    test "requires the key, the kind and the direction" do
      changeset = CustodyEvent.changeset(%CustodyEvent{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:kind] == ["can't be blank"]
      assert errors[:direction] == ["can't be blank"]
    end

    test "rejects a kind outside the custody flow" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:kind, :token_transfer)

      changeset = CustodyEvent.changeset(%CustodyEvent{}, attrs)

      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:kind]
    end

    test "rejects a direction outside deposit, withdrawal and balance delta" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:direction, :sideways)

      changeset = CustodyEvent.changeset(%CustodyEvent{}, attrs)

      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:direction]
    end

    test "rejects a negative amount" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:amount, Decimal.new(-1))

      changeset = CustodyEvent.changeset(%CustodyEvent{}, attrs)

      refute changeset.valid?
      assert changeset_errors(changeset)[:amount] == ["must be greater than or equal to 0"]
    end
  end

  describe "run/3" do
    test "inserts one row per custody log of the flow" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changes = [
        attrs,
        Map.merge(attrs, %{log_index: 6, kind: :claim_queued, direction: :withdrawal}),
        Map.merge(attrs, %{log_index: 7, kind: :custody_release, direction: :withdrawal})
      ]

      assert {:ok, %{insert_paxeer_x_custody_events: inserted}} = run_changes(changes)

      assert length(inserted) == 3

      assert [:custody_deposit, :claim_queued, :custody_release] ==
               CustodyEvent
               |> order_by([event], asc: event.log_index)
               |> select([event], event.kind)
               |> Repo.all()
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_custody_events: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_custody_events: []}} =
               run_changes([Map.merge(attrs, %{direction: :withdrawal, amount: Decimal.new(1)})])

      assert [stored] = Repo.all(CustodyEvent)
      assert stored.direction == :deposit
      assert Decimal.equal?(stored.amount, Decimal.new(1_000_000))
    end

    test "defaults the denormalized consensus flag of a fresh row to true" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.delete(:block_consensus)

      assert {:ok, %{insert_paxeer_x_custody_events: [inserted]}} = run_changes([attrs])
      assert inserted.block_consensus
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_custody_events: []}} = run_changes([])
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

      assert [kept] = Repo.all(CustodyEvent.only_consensus_query())
      assert kept.transaction_hash == kept_transaction.hash

      assert {1, nil} =
               [reorged_block.hash]
               |> CustodyEvent.lose_consensus_query()
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
      log_index: 5,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      kind: :custody_deposit,
      direction: :deposit,
      reference_id: block_hash(),
      asset_id: block_hash(),
      amount: Decimal.new(1_000_000),
      address_hash: address_hash(),
      account: block_hash()
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> CustodyEvents.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
