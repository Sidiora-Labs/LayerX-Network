defmodule Explorer.Chain.PaxeerX.GuarantorEventTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.GuarantorEvents
  alias Explorer.Chain.PaxeerX.GuarantorEvent

  describe "changeset/2" do
    test "accepts a registration with its signer, its operator and its bond" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changeset = GuarantorEvent.changeset(%GuarantorEvent{}, attrs)

      assert changeset.valid?
      assert {:ok, event} = Repo.insert(changeset)
      assert event.kind == :guarantor_registered
      assert event.event_name == "GuarantorRegistered"
      assert event.guarantor_id == attrs.guarantor_id
      assert event.signer_address_hash == attrs.signer_address_hash
      assert event.operator_address_hash == attrs.operator_address_hash
      assert Decimal.equal?(event.bond, Decimal.new(32_000_000_000_000_000_000))
      assert event.parameters["status"] == 1
      assert event.block_number == block.number
      assert event.block_consensus == true
    end

    test "requires the key, the block and the kind" do
      changeset = GuarantorEvent.changeset(%GuarantorEvent{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:kind] == ["can't be blank"]
    end

    test "accepts every kind the anchor precompile emits and rejects one it does not" do
      %{block: block, transaction: transaction} = block_with_transaction()

      assert GuarantorEvent.kinds() == [
               :guarantor_registered,
               :guarantor_activated,
               :guarantor_slashed,
               :bond_increased,
               :unbond_begun,
               :unbond_completed,
               :challenge_opened,
               :challenge_resolved,
               :availability_attested,
               :sequencer_authorized
             ]

      for {kind, index} <- Enum.with_index(GuarantorEvent.kinds(), 1) do
        attrs = block |> attributes(transaction) |> Map.merge(%{kind: kind, log_index: index})

        changeset = GuarantorEvent.changeset(%GuarantorEvent{}, attrs)

        assert changeset.valid?, "the schema rejected #{kind}"
        assert {:ok, stored} = Repo.insert(changeset)
        assert stored.kind == kind
      end

      assert Repo.aggregate(GuarantorEvent, :count) == length(GuarantorEvent.kinds())

      rejected =
        GuarantorEvent.changeset(
          %GuarantorEvent{},
          block |> attributes(transaction) |> Map.put(:kind, :guarantor_retired)
        )

      refute rejected.valid?
      assert {"is invalid", _} = rejected.errors[:kind]
    end

    test "rejects a negative bond and a mask wider than the precompile emits" do
      %{block: block, transaction: transaction} = block_with_transaction()

      negative =
        GuarantorEvent.changeset(
          %GuarantorEvent{},
          block |> attributes(transaction) |> Map.put(:bond, Decimal.new(-1))
        )

      refute negative.valid?
      assert changeset_errors(negative)[:bond] == ["must be greater than or equal to 0"]

      wide =
        GuarantorEvent.changeset(
          %GuarantorEvent{},
          block |> attributes(transaction) |> Map.merge(%{kind: :availability_attested, availability_mask: 256})
        )

      refute wide.valid?
      assert changeset_errors(wide)[:availability_mask] == ["must be less than or equal to 255"]
    end
  end

  describe "run/3" do
    test "stores the columns the challenge, availability, sequencer and unbonding events supply" do
      %{block: block, transaction: transaction} = block_with_transaction()

      key =
        block
        |> attributes(transaction)
        |> Map.take(~w(transaction_hash block_hash block_number block_consensus)a)

      guarantor_id = block_hash()
      evidence_hash = block_hash()
      challenger = address_hash()
      reporter = address_hash()
      sequencer_id = block_hash()
      sequencer_public_key = block_hash()

      changes = [
        Map.merge(key, %{
          log_index: 1,
          kind: :guarantor_slashed,
          event_name: "GuarantorSlashed",
          parameters: %{"reason" => 2},
          guarantor_id: guarantor_id,
          batch_number: 8801,
          amount: Decimal.new(4_000_000_000_000_000_000),
          reporter_address_hash: reporter,
          reporter_reward: Decimal.new(400_000_000_000_000_000)
        }),
        Map.merge(key, %{
          log_index: 2,
          kind: :challenge_opened,
          event_name: "ChallengeOpened",
          challenge_id: 77,
          batch_number: 8801,
          challenge_kind: 3,
          evidence_hash: evidence_hash,
          challenger_address_hash: challenger
        }),
        Map.merge(key, %{
          log_index: 3,
          kind: :challenge_resolved,
          event_name: "ChallengeResolved",
          challenge_id: 77,
          batch_number: 8801,
          upheld: true
        }),
        Map.merge(key, %{
          log_index: 4,
          kind: :availability_attested,
          event_name: "AvailabilityAttested",
          batch_number: 8801,
          guarantor_id: guarantor_id,
          class_mask: 7,
          availability_mask: 5
        }),
        Map.merge(key, %{
          log_index: 5,
          kind: :sequencer_authorized,
          event_name: "SequencerAuthorized",
          sequencer_id: sequencer_id,
          sequencer_public_key: sequencer_public_key,
          first_batch_number: 8800,
          last_batch_number: 8900
        }),
        Map.merge(key, %{
          log_index: 6,
          kind: :unbond_begun,
          event_name: "UnbondBegun",
          guarantor_id: guarantor_id,
          amount: Decimal.new(8_000_000_000_000_000_000),
          completion_time: 1_760_000_000
        })
      ]

      assert {:ok, %{insert_paxeer_x_guarantor_events: inserted}} = run_changes(changes)
      assert length(inserted) == 6

      assert [slashed, opened, resolved, attested, authorized, unbonding] =
               Repo.all(from(event in GuarantorEvent, order_by: [asc: event.log_index]))

      assert slashed.batch_number == 8801
      assert slashed.guarantor_id == guarantor_id
      assert Decimal.equal?(slashed.amount, Decimal.new(4_000_000_000_000_000_000))
      assert slashed.reporter_address_hash == reporter
      assert Decimal.equal?(slashed.reporter_reward, Decimal.new(400_000_000_000_000_000))
      assert slashed.parameters["reason"] == 2

      assert opened.challenge_id == 77
      assert opened.challenge_kind == 3
      assert opened.evidence_hash == evidence_hash
      assert opened.challenger_address_hash == challenger

      assert resolved.challenge_id == 77
      assert resolved.upheld == true

      assert attested.class_mask == 7
      assert attested.availability_mask == 5
      assert attested.guarantor_id == guarantor_id

      assert authorized.sequencer_id == sequencer_id
      assert authorized.sequencer_public_key == sequencer_public_key
      assert authorized.first_batch_number == 8800
      assert authorized.last_batch_number == 8900

      assert Decimal.equal?(unbonding.amount, Decimal.new(8_000_000_000_000_000_000))
      assert unbonding.completion_time == 1_760_000_000
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_guarantor_events: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_guarantor_events: []}} =
               run_changes([Map.merge(attrs, %{kind: :guarantor_slashed, bond: Decimal.new(1)})])

      assert [stored] = Repo.all(GuarantorEvent)
      assert stored.kind == :guarantor_registered
      assert Decimal.equal?(stored.bond, Decimal.new(32_000_000_000_000_000_000))
    end

    test "replaying a whole block keeps one row per log" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      changes = [
        attrs,
        Map.merge(attrs, %{log_index: 2, kind: :guarantor_activated, event_name: "GuarantorActivated"}),
        Map.merge(attrs, %{
          log_index: 3,
          kind: :bond_increased,
          event_name: "BondIncreased",
          amount: Decimal.new(1_000)
        })
      ]

      assert {:ok, %{insert_paxeer_x_guarantor_events: first}} = run_changes(changes)
      assert length(first) == 3

      assert {:ok, %{insert_paxeer_x_guarantor_events: []}} = run_changes(changes)

      assert Repo.aggregate(GuarantorEvent, :count) == 3

      assert [:guarantor_registered, :guarantor_activated, :bond_increased] ==
               Repo.all(from(event in GuarantorEvent, order_by: [asc: event.log_index], select: event.kind))
    end

    test "writes a row for the block a reorg moved the transaction into" do
      %{block: reorged_block, transaction: transaction} = block_with_transaction()
      replacement_block = insert(:block)

      attrs = attributes(reorged_block, transaction)

      replayed =
        Map.merge(attrs, %{block_hash: replacement_block.hash, block_number: replacement_block.number})

      assert {:ok, %{insert_paxeer_x_guarantor_events: [_]}} = run_changes([attrs])
      assert {:ok, %{insert_paxeer_x_guarantor_events: [_]}} = run_changes([replayed])

      assert Repo.aggregate(GuarantorEvent, :count) == 2

      assert Enum.sort([reorged_block.hash, replacement_block.hash]) ==
               GuarantorEvent |> select([event], event.block_hash) |> Repo.all() |> Enum.sort()

      Repo.update!(Ecto.Changeset.change(reorged_block, consensus: false))

      assert [%GuarantorEvent{block_hash: kept}] = Repo.all(GuarantorEvent.only_consensus_query())
      assert kept == replacement_block.hash
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_guarantor_events: []}} = run_changes([])
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

      assert Repo.aggregate(GuarantorEvent.only_consensus_query(), :count) == 1

      assert {1, nil} =
               [reorged_block.hash]
               |> GuarantorEvent.lose_consensus_query()
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
      kind: :guarantor_registered,
      event_name: "GuarantorRegistered",
      parameters: %{"status" => 1},
      guarantor_id: block_hash(),
      signer_address_hash: address_hash(),
      operator_address_hash: address_hash(),
      bond: Decimal.new(32_000_000_000_000_000_000)
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> GuarantorEvents.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
