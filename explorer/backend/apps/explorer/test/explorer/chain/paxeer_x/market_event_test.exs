defmodule Explorer.Chain.PaxeerX.MarketEventTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.MarketEvents
  alias Explorer.Chain.PaxeerX.MarketEvent

  describe "changeset/2" do
    test "accepts an exchange action" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset = MarketEvent.changeset(%MarketEvent{}, attributes(block, transaction))

      assert changeset.valid?
      assert {:ok, event} = Repo.insert(changeset)
      assert event.domain == :exchange
      assert event.kind == "OrderPlaced"
      assert Decimal.equal?(event.amount, Decimal.new(2500))
      assert event.block_number == block.number
    end

    test "accepts a bridge action that names the asset by its EVM address" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs =
        block
        |> attributes(transaction)
        |> Map.merge(%{domain: :bridge, kind: "BridgeIn", asset_id: nil, asset_address_hash: address_hash()})

      assert {:ok, event} = %MarketEvent{} |> MarketEvent.changeset(attrs) |> Repo.insert()
      assert event.domain == :bridge
      assert is_nil(event.asset_id)
      refute is_nil(event.asset_address_hash)
    end

    test "accepts a launchpad action that moves no amount" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs =
        block
        |> attributes(transaction)
        |> Map.merge(%{domain: :launchpad, kind: "PauseToggled", amount: nil})

      assert {:ok, event} = %MarketEvent{} |> MarketEvent.changeset(attrs) |> Repo.insert()
      assert event.domain == :launchpad
      assert is_nil(event.amount)
    end

    test "requires the key, the domain and the kind" do
      changeset = MarketEvent.changeset(%MarketEvent{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:domain] == ["can't be blank"]
      assert errors[:kind] == ["can't be blank"]
    end

    test "rejects a domain outside exchange, bridge and launchpad" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:domain, :custody)

      changeset = MarketEvent.changeset(%MarketEvent{}, attrs)

      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:domain]
      assert MarketEvent.domains() == [:exchange, :bridge, :launchpad]
    end

    test "rejects an empty kind" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:kind, "")

      changeset = MarketEvent.changeset(%MarketEvent{}, attrs)

      refute changeset.valid?
      assert changeset_errors(changeset)[:kind] == ["can't be blank"]
    end
  end

  describe "run/3" do
    test "inserts one row per market log across the three domains" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changes = [
        attrs,
        Map.merge(attrs, %{log_index: 11, domain: :bridge, kind: "BridgeOut"}),
        Map.merge(attrs, %{log_index: 12, domain: :launchpad, kind: "Swap"})
      ]

      assert {:ok, %{insert_paxeer_x_market_events: inserted}} = run_changes(changes)

      assert length(inserted) == 3

      assert [{:exchange, "OrderPlaced"}, {:bridge, "BridgeOut"}, {:launchpad, "Swap"}] ==
               MarketEvent
               |> order_by([event], asc: event.log_index)
               |> select([event], {event.domain, event.kind})
               |> Repo.all()
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_market_events: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_market_events: []}} =
               run_changes([Map.merge(attrs, %{kind: "OrderCancelRequested", amount: Decimal.new(1)})])

      assert [stored] = Repo.all(MarketEvent)
      assert stored.kind == "OrderPlaced"
      assert Decimal.equal?(stored.amount, Decimal.new(2500))
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_market_events: []}} = run_changes([])
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

      assert [kept] = Repo.all(MarketEvent.only_consensus_query())
      assert kept.transaction_hash == kept_transaction.hash

      assert {1, nil} =
               [reorged_block.hash]
               |> MarketEvent.lose_consensus_query()
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
      log_index: 10,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      domain: :exchange,
      kind: "OrderPlaced",
      address_hash: address_hash(),
      account: block_hash(),
      asset_id: block_hash(),
      asset_address_hash: address_hash(),
      amount: Decimal.new(2500)
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> MarketEvents.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
