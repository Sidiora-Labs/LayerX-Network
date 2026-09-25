defmodule Indexer.Transform.PaxeerXLogsTest do
  use ExUnit.Case, async: true

  alias ABI.TypeEncoder
  alias Explorer.Chain.PaxeerX.{AccountBinding, Anchor, CustodyEvent, DepositRoot, GuarantorEvent, MarketEvent, Receipt}
  alias Indexer.Transform.PaxeerXLogs

  @addr_precompile "0x0000000000000000000000000000000000001004"
  @custody_precompile "0x0000000000000000000000000000000000001013"
  @anchor_precompile "0x0000000000000000000000000000000000001014"
  @exchange_precompile "0x0000000000000000000000000000000000001015"
  @bridge_precompile "0x0000000000000000000000000000000000001016"
  @launchpad_precompile "0x0000000000000000000000000000000000001017"

  @erc20_transfer_topic "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"

  @schemas %{
    lx_account_bindings: AccountBinding,
    lx_custody_events: CustodyEvent,
    lx_deposit_roots: DepositRoot,
    lx_anchors: Anchor,
    lx_guarantor_events: GuarantorEvent,
    lx_market_events: MarketEvent,
    lx_receipts: Receipt
  }

  @empty %{
    lx_account_bindings: [],
    lx_custody_events: [],
    lx_deposit_roots: [],
    lx_anchors: [],
    lx_guarantor_events: [],
    lx_receipts: [],
    lx_market_events: [],
    undecoded_log_count: 0
  }

  @row_tables [
    :lx_account_bindings,
    :lx_custody_events,
    :lx_deposit_roots,
    :lx_anchors,
    :lx_guarantor_events,
    :lx_market_events
  ]

  describe "definitions/0" do
    test "every topic0 constant is keccak256 of the event signature from the precompile ABI" do
      for definition <- PaxeerXLogs.definitions() do
        assert definition.signature == canonical_signature(definition),
               "#{definition.name} signature does not match its argument list"

        assert definition.topic == "0x" <> Base.encode16(ExKeccak.hash_256(definition.signature), case: :lower),
               "#{definition.name} topic0 is not keccak256(#{definition.signature})"
      end
    end

    test "every event is attributed to the precompile that emits it" do
      by_table = Enum.group_by(PaxeerXLogs.definitions(), & &1.table, & &1.address)

      assert by_table |> Map.fetch!(:lx_account_bindings) |> Enum.uniq() == [@addr_precompile]
      assert by_table |> Map.fetch!(:lx_custody_events) |> Enum.uniq() == [@custody_precompile]
      assert by_table |> Map.fetch!(:lx_deposit_roots) |> Enum.uniq() == [@custody_precompile]
      assert by_table |> Map.fetch!(:lx_anchors) |> Enum.uniq() == [@anchor_precompile]
      assert by_table |> Map.fetch!(:lx_guarantor_events) |> Enum.uniq() == [@anchor_precompile]

      assert by_table |> Map.fetch!(:lx_market_events) |> Enum.uniq() |> Enum.sort() == [
               @exchange_precompile,
               @bridge_precompile,
               @launchpad_precompile
             ]
    end

    test "no two events share an emitter and a topic0" do
      keys = Enum.map(PaxeerXLogs.definitions(), &{&1.address, &1.topic})

      assert keys == Enum.uniq(keys)
    end

    test "no event targets lx_receipts: a kernel receipt is not an EVM event" do
      refute Enum.any?(PaxeerXLogs.definitions(), &(&1.table == :lx_receipts))
    end

    test "the custody definitions cover the kinds and directions the schema declares" do
      custody = Enum.filter(PaxeerXLogs.definitions(), &(&1.table == :lx_custody_events))

      assert custody |> Enum.map(& &1.kind) |> Enum.sort() == Enum.sort(CustodyEvent.kinds())

      for definition <- custody do
        assert definition.direction in CustodyEvent.directions(),
               "#{definition.name} carries a direction the schema does not declare"
      end
    end

    test "the anchor definitions carry a status the schema declares" do
      statuses =
        PaxeerXLogs.definitions()
        |> Enum.filter(&(&1.table == :lx_anchors))
        |> Enum.map(& &1.status)

      assert statuses == [:submitted, :final]
      assert Enum.all?(statuses, &(&1 in Anchor.statuses()))
    end

    test "the guarantor definitions cover the kinds the schema declares" do
      guarantor = Enum.filter(PaxeerXLogs.definitions(), &(&1.table == :lx_guarantor_events))

      assert guarantor |> Enum.map(& &1.kind) |> Enum.sort() == Enum.sort(GuarantorEvent.kinds())
      assert length(guarantor) == 10
    end

    test "the deposit root registration is the only definition of its table" do
      assert [definition] = Enum.filter(PaxeerXLogs.definitions(), &(&1.table == :lx_deposit_roots))

      assert definition.name == "DepositRootRegistered"

      assert definition.arguments |> Enum.map(fn {name, _type, _indexed?} -> name end) |> Enum.sort() ==
               ~w(checkpointId commitment depositRoot version)
    end

    test "every anchor and custody precompile event of the committed ABI has a definition" do
      by_address = Enum.group_by(PaxeerXLogs.definitions(), & &1.address, & &1.name)

      assert by_address |> Map.fetch!(@anchor_precompile) |> Enum.sort() ==
               ~w(AvailabilityAttested BondIncreased ChallengeOpened ChallengeResolved CheckpointFinalized
                  CheckpointSubmitted GuarantorActivated GuarantorRegistered GuarantorSlashed SequencerAuthorized
                  UnbondBegun UnbondCompleted)

      assert by_address |> Map.fetch!(@custody_precompile) |> Enum.sort() ==
               ~w(ClaimFinalised ClaimQueued CustodyDeposit CustodyRelease DepositRootRegistered
                  EmergencyExitExecuted)
    end

    test "the market definitions cover the domains the schema declares" do
      market = Enum.filter(PaxeerXLogs.definitions(), &(&1.table == :lx_market_events))

      assert market |> Enum.map(& &1.domain) |> Enum.uniq() |> Enum.sort() == Enum.sort(MarketEvent.domains())

      for definition <- market do
        assert definition.kind == Macro.underscore(definition.name),
               "#{definition.name} has a kind that is not the underscore spelling of its event name"
      end
    end
  end

  describe "parse/1" do
    test "decodes one ABI encoded log per precompile event into a row its schema accepts" do
      fixtures = Enum.map(Enum.with_index(PaxeerXLogs.definitions()), &fixture/1)

      result = fixtures |> Enum.map(& &1.log) |> PaxeerXLogs.parse()

      assert Map.keys(result) |> Enum.sort() == Enum.sort(Map.keys(@empty))

      rows = Enum.flat_map(@row_tables, &result[&1])
      assert length(rows) == length(fixtures)
      assert result.undecoded_log_count == 0

      for %{definition: definition, log: log, arguments: arguments} <- fixtures do
        row =
          result
          |> Map.fetch!(definition.table)
          |> Enum.find(&(&1.log_index == log.index))

        assert row, "#{definition.name} produced no #{definition.table} row"
        assert row.event_name == definition.name
        assert row.transaction_hash == log.transaction_hash
        assert row.block_hash == log.block_hash
        assert row.block_number == log.block_number
        assert row.parameters == arguments

        assert_schema_row(definition.table, row)
      end
    end

    test "a binding log fills the promoted account binding columns" do
      definition = definition!("LayerXBound")

      log =
        log(definition, 12, %{
          "evm" => <<0x11::160>>,
          "didPublicKey" => <<0x22::256>>,
          "nonce" => 7
        })

      assert %{lx_account_bindings: [row]} = PaxeerXLogs.parse([log])

      assert row.evm_address_hash == "0x0000000000000000000000000000000000000011"
      assert row.layerx_did == "did:layerx:0000000000000000000000000000000000000000000000000000000000000022"
      assert row.nonce == 7
      assert row.bound == true
      assert row.event_name == "LayerXBound"
      assert row.log_index == 12
      assert row.parameters["didPublicKey"] == "0x0000000000000000000000000000000000000000000000000000000000000022"

      assert_schema_row(:lx_account_bindings, row)
    end

    test "an unbinding log is the same row with bound false" do
      definition = definition!("LayerXUnbound")

      log =
        log(definition, 13, %{
          "evm" => <<0x11::160>>,
          "didPublicKey" => <<0x22::256>>,
          "nonce" => 8
        })

      assert %{lx_account_bindings: [row]} = PaxeerXLogs.parse([log])

      assert row.bound == false
      assert row.event_name == "LayerXUnbound"

      assert_schema_row(:lx_account_bindings, row)
    end

    test "a custody deposit log is a deposit of the payer, not of the precompile" do
      definition = definition!("CustodyDeposit")

      log =
        log(definition, 3, %{
          "depositId" => <<0xAB::256>>,
          "assetId" => <<0xCD::256>>,
          "payer" => <<0xEF::160>>,
          "beneficiary" => <<0x99::256>>,
          "amount" => 1_500_000_000_000_000_000,
          "nonce" => 42
        })

      assert %{lx_custody_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :custody_deposit
      assert row.direction == :deposit
      assert row.reference_id == "0x00000000000000000000000000000000000000000000000000000000000000ab"
      assert row.asset_id == "0x00000000000000000000000000000000000000000000000000000000000000cd"
      assert row.address_hash == "0x00000000000000000000000000000000000000ef"
      assert row.account == "0x0000000000000000000000000000000000000000000000000000000000000099"
      assert row.amount == 1_500_000_000_000_000_000
      assert row.nullifier == nil
      assert row.checkpoint_hash == nil
      assert row.parameters["nonce"] == 42

      assert_schema_row(:lx_custody_events, row)
    end

    test "a claim queued log keeps the claim, the nullifier and the checkpoint apart" do
      definition = definition!("ClaimQueued")

      log =
        log(definition, 4, %{
          "claimId" => <<0x01::256>>,
          "nullifier" => <<0x02::256>>,
          "checkpointHash" => <<0x03::256>>,
          "assetId" => <<0x04::256>>,
          "recipient" => <<0x05::160>>,
          "amount" => 250,
          "availableAt" => 1_700_000_000
        })

      assert %{lx_custody_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :claim_queued
      assert row.direction == :withdrawal
      assert row.reference_id == bytes32(1)
      assert row.nullifier == bytes32(2)
      assert row.checkpoint_hash == bytes32(3)
      assert row.asset_id == bytes32(4)
      assert row.address_hash == address(5)
      assert row.amount == 250
      assert row.parameters["availableAt"] == 1_700_000_000

      assert_schema_row(:lx_custody_events, row)
    end

    test "a claim finalisation is a balance delta that carries only its linkage" do
      definition = definition!("ClaimFinalised")

      log = log(definition, 2, %{"claimId" => <<0x01::256>>, "nullifier" => <<0x02::256>>})

      assert %{lx_custody_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :claim_finalised
      assert row.direction == :balance_delta
      assert row.reference_id == bytes32(1)
      assert row.nullifier == bytes32(2)
      assert row.amount == nil
      assert row.address_hash == nil

      assert_schema_row(:lx_custody_events, row)
    end

    test "an emergency exit is a withdrawal of its recipient crediting its kernel account" do
      definition = definition!("EmergencyExitExecuted")

      log =
        log(definition, 14, %{
          "claimId" => <<0x01::256>>,
          "nullifier" => <<0x02::256>>,
          "checkpointHash" => <<0x03::256>>,
          "account" => <<0x04::256>>,
          "assetId" => <<0x05::256>>,
          "recipient" => <<0x06::160>>,
          "amount" => 9
        })

      assert %{lx_custody_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :emergency_exit
      assert row.direction == :withdrawal
      assert row.account == bytes32(4)
      assert row.address_hash == address(6)

      assert_schema_row(:lx_custody_events, row)
    end

    test "a checkpoint submission fills the promoted anchor columns with the submitted status" do
      definition = definition!("CheckpointSubmitted")

      log =
        log(definition, 0, %{
          "batchNumber" => 91_234,
          "checkpointId" => <<0x7A::256>>,
          "stateRoot" => <<0x7B::256>>,
          "receiptRoot" => <<0x7C::256>>,
          "signers" => 9
        })

      assert %{lx_anchors: [row]} = PaxeerXLogs.parse([log])

      assert row.status == :submitted
      assert row.batch_number == 91_234
      assert row.checkpoint_id == bytes32(0x7A)
      assert row.state_root == bytes32(0x7B)
      assert row.receipt_root == bytes32(0x7C)
      assert row.signers == 9

      assert_schema_row(:lx_anchors, row)
    end

    test "a checkpoint finalization is the same checkpoint with the final status" do
      definition = definition!("CheckpointFinalized")

      log =
        log(definition, 1, %{
          "batchNumber" => 91_234,
          "checkpointId" => <<0x7A::256>>,
          "stateRoot" => <<0x7B::256>>,
          "receiptRoot" => <<0x7C::256>>
        })

      assert %{lx_anchors: [row]} = PaxeerXLogs.parse([log])

      assert row.status == :final
      assert row.batch_number == 91_234
      assert row.checkpoint_id == bytes32(0x7A)
      assert row.signers == nil

      assert_schema_row(:lx_anchors, row)
    end

    test "a guarantor registration fills the signer, the operator and the bond" do
      definition = definition!("GuarantorRegistered")

      log =
        log(definition, 20, %{
          "guarantorId" => <<0x1A::256>>,
          "signer" => <<0x21::160>>,
          "operator" => <<0x22::160>>,
          "bond" => 5_000_000_000_000_000_000,
          "status" => 1
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :guarantor_registered
      assert row.event_name == "GuarantorRegistered"
      assert row.log_index == 20
      assert row.guarantor_id == bytes32(0x1A)
      assert row.signer_address_hash == address(0x21)
      assert row.operator_address_hash == address(0x22)
      assert row.bond == 5_000_000_000_000_000_000
      assert row.amount == nil
      assert row.batch_number == nil
      assert row.challenge_id == nil
      assert row.parameters["status"] == 1

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a guarantor activation carries its guarantor and nothing else" do
      definition = definition!("GuarantorActivated")

      log = log(definition, 21, %{"guarantorId" => <<0x1A::256>>})

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :guarantor_activated
      assert row.guarantor_id == bytes32(0x1A)
      assert row.bond == nil
      assert row.amount == nil
      assert row.signer_address_hash == nil
      assert row.parameters == %{"guarantorId" => bytes32(0x1A)}

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a slash records the slashed value, the batch and the reporter's reward" do
      definition = definition!("GuarantorSlashed")

      log =
        log(definition, 22, %{
          "guarantorId" => <<0x1A::256>>,
          "reason" => 2,
          "batchNumber" => 91_234,
          "amount" => 4_000_000_000_000_000_000,
          "reporter" => <<0x31::160>>,
          "reporterReward" => 400_000_000_000_000_000
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :guarantor_slashed
      assert row.guarantor_id == bytes32(0x1A)
      assert row.batch_number == 91_234
      assert row.amount == 4_000_000_000_000_000_000
      assert row.reporter_address_hash == address(0x31)
      assert row.reporter_reward == 400_000_000_000_000_000
      assert row.bond == nil
      assert row.parameters["reason"] == 2

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a bond increase keeps the moved value apart from the resulting bond" do
      definition = definition!("BondIncreased")

      log =
        log(definition, 23, %{
          "guarantorId" => <<0x1A::256>>,
          "amount" => 1_000_000_000_000_000_000,
          "bond" => 6_000_000_000_000_000_000
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :bond_increased
      assert row.guarantor_id == bytes32(0x1A)
      assert row.amount == 1_000_000_000_000_000_000
      assert row.bond == 6_000_000_000_000_000_000
      assert row.completion_time == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a begun unbonding carries the value and the moment it becomes claimable" do
      definition = definition!("UnbondBegun")

      log =
        log(definition, 24, %{
          "guarantorId" => <<0x1A::256>>,
          "amount" => 2_000_000_000_000_000_000,
          "completionTime" => 1_700_000_000
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :unbond_begun
      assert row.guarantor_id == bytes32(0x1A)
      assert row.amount == 2_000_000_000_000_000_000
      assert row.completion_time == 1_700_000_000

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a completed unbonding is the same guarantor without a completion time" do
      definition = definition!("UnbondCompleted")

      log = log(definition, 25, %{"guarantorId" => <<0x1A::256>>, "amount" => 2_000_000_000_000_000_000})

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :unbond_completed
      assert row.guarantor_id == bytes32(0x1A)
      assert row.amount == 2_000_000_000_000_000_000
      assert row.completion_time == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "an opened challenge keeps its kind, its evidence and its challenger" do
      definition = definition!("ChallengeOpened")

      log =
        log(definition, 26, %{
          "challengeId" => 77,
          "batchNumber" => 91_234,
          "kind" => 3,
          "evidenceHash" => <<0x4A::256>>,
          "challenger" => <<0x4B::160>>
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :challenge_opened
      assert row.challenge_id == 77
      assert row.batch_number == 91_234
      assert row.challenge_kind == 3
      assert row.evidence_hash == bytes32(0x4A)
      assert row.challenger_address_hash == address(0x4B)
      assert row.guarantor_id == nil
      assert row.upheld == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a resolved challenge records whether it was upheld" do
      definition = definition!("ChallengeResolved")

      log = log(definition, 27, %{"challengeId" => 77, "batchNumber" => 91_234, "upheld" => true})

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :challenge_resolved
      assert row.challenge_id == 77
      assert row.batch_number == 91_234
      assert row.upheld == true
      assert row.evidence_hash == nil
      assert row.challenger_address_hash == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "an availability attestation keeps the classes it covers and the ones it reports available" do
      definition = definition!("AvailabilityAttested")

      log =
        log(definition, 28, %{
          "batchNumber" => 91_234,
          "guarantorId" => <<0x5E::256>>,
          "classMask" => 3,
          "availabilityMask" => 1
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :availability_attested
      assert row.batch_number == 91_234
      assert row.guarantor_id == bytes32(0x5E)
      assert row.class_mask == 3
      assert row.availability_mask == 1
      assert row.amount == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a sequencer authorisation carries its public key and the batch range it covers" do
      definition = definition!("SequencerAuthorized")

      log =
        log(definition, 29, %{
          "sequencerId" => <<0x6A::256>>,
          "publicKey" => <<0x6B::256>>,
          "firstBatchNumber" => 91_000,
          "lastBatchNumber" => 92_000
        })

      assert %{lx_guarantor_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == :sequencer_authorized
      assert row.sequencer_id == bytes32(0x6A)
      assert row.sequencer_public_key == bytes32(0x6B)
      assert row.first_batch_number == 91_000
      assert row.last_batch_number == 92_000
      assert row.guarantor_id == nil

      assert_schema_row(:lx_guarantor_events, row)
    end

    test "a deposit root registration is a deposit root, not a custody movement" do
      definition = definition!("DepositRootRegistered")

      log =
        log(definition, 30, %{
          "checkpointId" => <<0x7A::256>>,
          "depositRoot" => <<0x7B::256>>,
          "commitment" => <<0x7C::256>>,
          "version" => 1
        })

      assert %{lx_deposit_roots: [row], lx_custody_events: []} = PaxeerXLogs.parse([log])

      assert row.event_name == "DepositRootRegistered"
      assert row.log_index == 30
      assert row.checkpoint_id == bytes32(0x7A)
      assert row.deposit_root == bytes32(0x7B)
      assert row.commitment == bytes32(0x7C)
      assert row.version == 1
      assert row.parameters["version"] == 1

      assert_schema_row(:lx_deposit_roots, row)
    end

    test "an exchange log is a market event of the exchange domain" do
      definition = definition!("OrderPlaced")

      log =
        log(definition, 5, %{
          "intentId" => <<0x21::256>>,
          "marketId" => <<0x22::256>>,
          "owner" => <<0x23::160>>,
          "side" => 1,
          "price" => 42_000_000,
          "quantity" => 3,
          "timeInForce" => 2,
          "nonce" => 11
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.domain == :exchange
      assert row.kind == "order_placed"
      assert row.address_hash == address(0x23)
      assert row.amount == nil
      assert row.asset_address_hash == nil
      assert row.parameters["marketId"] == bytes32(0x22)
      assert row.parameters["price"] == 42_000_000
      assert row.parameters["nonce"] == 11

      assert_schema_row(:lx_market_events, row)
    end

    test "a margin deposit keeps both the EVM owner and the kernel account" do
      definition = definition!("MarginDeposited")

      log =
        log(definition, 15, %{
          "intentId" => <<0x31::256>>,
          "account" => <<0x32::256>>,
          "owner" => <<0x33::160>>,
          "assetId" => <<0x34::256>>,
          "amount" => 4_000,
          "depositId" => <<0x35::256>>,
          "nonce" => 3
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == "margin_deposited"
      assert row.address_hash == address(0x33)
      assert row.account == bytes32(0x32)
      assert row.asset_id == bytes32(0x34)
      assert row.amount == 4_000

      assert_schema_row(:lx_market_events, row)
    end

    test "a bridge log decodes its dynamic denom and keeps the source chain" do
      definition = definition!("BridgeIn")

      log =
        log(definition, 6, %{
          "chain" => 125,
          "txHash" => <<0x31::256>>,
          "recipient" => <<0x32::160>>,
          "logIndex" => 4,
          "asset" => <<0x33::160>>,
          "amount" => 10_000,
          "denom" => "ulxbtc"
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.domain == :bridge
      assert row.kind == "bridge_in"
      assert row.address_hash == address(0x32)
      assert row.asset_address_hash == address(0x33)
      assert row.amount == 10_000
      assert row.parameters["chain"] == 125
      assert row.parameters["denom"] == "ulxbtc"
      assert row.parameters["logIndex"] == 4

      assert_schema_row(:lx_market_events, row)
    end

    test "a bridge out log reads its nonce from the third topic" do
      definition = definition!("BridgeOut")

      log =
        log(definition, 7, %{
          "chain" => 1,
          "asset" => <<0x41::160>>,
          "amount" => 77,
          "recipient" => <<0x42::160>>,
          "nonce" => 512
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.parameters["nonce"] == 512
      assert row.asset_address_hash == address(0x41)
      assert row.address_hash == address(0x42)
      assert row.amount == 77

      assert_schema_row(:lx_market_events, row)
    end

    test "a launchpad swap prefers the trader over the recipient as the acting address" do
      definition = definition!("Swap")

      log =
        log(definition, 8, %{
          "token" => <<0x51::160>>,
          "trader" => <<0x52::160>>,
          "recipient" => <<0x53::160>>,
          "isBuy" => true,
          "amountIn" => 100,
          "amountOut" => 98,
          "feeAmount" => 2,
          "price" => 1_020_000
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.domain == :launchpad
      assert row.kind == "swap"
      assert row.address_hash == address(0x52)
      assert row.asset_address_hash == address(0x51)
      assert row.amount == nil
      assert row.parameters["isBuy"] == true
      assert row.parameters["amountOut"] == 98

      assert_schema_row(:lx_market_events, row)
    end

    test "a launchpad market creation decodes its three dynamic strings" do
      definition = definition!("MarketCreated")

      log =
        log(definition, 9, %{
          "token" => <<0x61::160>>,
          "creator" => <<0x62::160>>,
          "denom" => "factory/pax1/demo",
          "name" => "Demo Market",
          "symbol" => "DEMO",
          "feeStrategy" => 2
        })

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == "market_created"
      assert row.address_hash == address(0x62)
      assert row.parameters["denom"] == "factory/pax1/demo"
      assert row.parameters["name"] == "Demo Market"
      assert row.parameters["symbol"] == "DEMO"
      assert row.parameters["feeStrategy"] == 2

      assert_schema_row(:lx_market_events, row)
    end

    test "a topic0 written in upper case still matches" do
      definition = definition!("FeesBurned")

      log =
        definition
        |> log(10, %{"token" => <<0x71::160>>, "amount" => 5})
        |> Map.update!(:first_topic, &String.upcase/1)

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.kind == "fees_burned"
      assert row.amount == 5

      assert_schema_row(:lx_market_events, row)
    end

    test "lx_receipts stays empty: a kernel receipt is not an EVM event" do
      fixtures = Enum.map(Enum.with_index(PaxeerXLogs.definitions()), &fixture/1)

      assert %{lx_receipts: []} = fixtures |> Enum.map(& &1.log) |> PaxeerXLogs.parse()
    end

    test "an empty log list yields empty lists" do
      assert PaxeerXLogs.parse([]) == @empty
    end

    test "a log that is not a precompile event yields nothing" do
      erc20_transfer = %{
        address_hash: "0xf2eec76e45b328df99a34fa696320a262cb92154",
        block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
        block_number: 3_530_917,
        data: "0x000000000000000000000000000000000000000000000000ebec21ee1da40000",
        first_topic: @erc20_transfer_topic,
        second_topic: "0x000000000000000000000000556813d9cc20acfe8388af029a679d34a63388db",
        third_topic: "0x00000000000000000000000092148dd870fa1b7c4700f2bd7f44238821c26f73",
        fourth_topic: nil,
        index: 8,
        transaction_hash: "0x43dfd761974e8c3351d285ab65bee311454eb45b149a015fe7804a33252f19e5"
      }

      assert PaxeerXLogs.parse([erc20_transfer]) == @empty
    end

    test "a precompile topic0 emitted by an ordinary contract yields nothing" do
      definition = definition!("CustodyDeposit")

      log =
        definition
        |> log(3, %{
          "depositId" => <<0xAB::256>>,
          "assetId" => <<0xCD::256>>,
          "payer" => <<0xEF::160>>,
          "beneficiary" => <<0x99::256>>,
          "amount" => 1,
          "nonce" => 1
        })
        |> Map.put(:address_hash, "0xf2eec76e45b328df99a34fa696320a262cb92154")

      assert PaxeerXLogs.parse([log]) == @empty
    end

    test "an unknown topic0 at a precompile address yields no row and one undecoded log" do
      definition = definition!("CustodyDeposit")

      log =
        definition
        |> log(3, %{
          "depositId" => <<0xAB::256>>,
          "assetId" => <<0xCD::256>>,
          "payer" => <<0xEF::160>>,
          "beneficiary" => <<0x99::256>>,
          "amount" => 1,
          "nonce" => 1
        })
        |> Map.put(:first_topic, @erc20_transfer_topic)

      assert PaxeerXLogs.parse([log]) == %{@empty | undecoded_log_count: 1}
    end

    test "the committed ABI set leaves no precompile log undecoded" do
      fixtures = Enum.map(Enum.with_index(PaxeerXLogs.definitions()), &fixture/1)

      result = fixtures |> Enum.map(& &1.log) |> PaxeerXLogs.parse()

      assert result.undecoded_log_count == 0
      assert length(Enum.flat_map(@row_tables, &result[&1])) == length(PaxeerXLogs.definitions())
    end

    test "a precompile log whose topic0 no definition matches is counted rather than dropped" do
      unknown = %{
        address_hash: @anchor_precompile,
        block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
        block_number: 42,
        data: "0x" <> Base.encode16(TypeEncoder.encode([5], [{:uint, 256}]), case: :lower),
        first_topic: "0x" <> Base.encode16(ExKeccak.hash_256("GuarantorRetired(bytes32,uint256)"), case: :lower),
        second_topic: bytes32(1),
        third_topic: nil,
        fourth_topic: nil,
        index: 0,
        transaction_hash: "0x43dfd761974e8c3351d285ab65bee311454eb45b149a015fe7804a33252f19e5"
      }

      decoded = log(definition!("GuarantorActivated"), 1, %{"guarantorId" => <<0x1A::256>>})

      result = PaxeerXLogs.parse([unknown, decoded])

      assert result.undecoded_log_count == 1
      assert [%{kind: :guarantor_activated}] = result.lx_guarantor_events
    end

    test "a log missing an indexed topic yields no row and one undecoded log" do
      definition = definition!("ClaimFinalised")

      log =
        definition
        |> log(2, %{"claimId" => <<0x01::256>>, "nullifier" => <<0x02::256>>})
        |> Map.put(:third_topic, nil)

      assert PaxeerXLogs.parse([log]) == %{@empty | undecoded_log_count: 1}
    end

    test "a log carrying no data for its unindexed arguments yields no row and one undecoded log" do
      definition = definition!("CheckpointFinalized")

      log =
        definition
        |> log(4, %{
          "batchNumber" => 9,
          "checkpointId" => <<0x07::256>>,
          "stateRoot" => <<0x08::256>>,
          "receiptRoot" => <<0x09::256>>
        })
        |> Map.put(:data, "0x")

      assert PaxeerXLogs.parse([log]) == %{@empty | undecoded_log_count: 1}
    end
  end

  defp assert_schema_row(table, row) do
    schema = Map.fetch!(@schemas, table)
    columns = MapSet.new(schema.__schema__(:fields))
    unknown = row |> Map.keys() |> MapSet.new() |> MapSet.difference(columns) |> MapSet.to_list()

    assert unknown == [], "#{table} row carries keys that are not columns of #{inspect(schema)}: #{inspect(unknown)}"
    assert is_binary(row.event_name)
    assert is_map(row.parameters) and map_size(row.parameters) > 0

    changeset = schema.changeset(struct(schema), row)

    assert changeset.valid?,
           "#{table} row #{inspect(row)} is rejected by #{inspect(schema)}: #{inspect(changeset.errors)}"

    changeset
  end

  defp definition!(name), do: Enum.find(PaxeerXLogs.definitions(), &(&1.name == name))

  defp bytes32(value), do: "0x" <> (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(64, "0"))

  defp address(value), do: "0x" <> (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(40, "0"))

  defp canonical_signature(%{name: name, arguments: arguments}) do
    name <> "(" <> Enum.map_join(arguments, ",", fn {_name, type, _indexed?} -> abi_type(type) end) <> ")"
  end

  defp abi_type(:address), do: "address"
  defp abi_type(:bool), do: "bool"
  defp abi_type(:string), do: "string"
  defp abi_type({:bytes, size}), do: "bytes#{size}"
  defp abi_type({:uint, bits}), do: "uint#{bits}"

  defp fixture({definition, index}) do
    values =
      definition.arguments
      |> Enum.with_index()
      |> Map.new(fn {{name, type, _indexed?}, position} ->
        {name, sample(type, index * 31 + position + 1)}
      end)

    %{
      definition: definition,
      log: log(definition, index, values),
      arguments: Map.new(values, fn {name, value} -> {name, expected(type_of(definition, name), value)} end)
    }
  end

  defp type_of(definition, name) do
    Enum.find_value(definition.arguments, fn {argument, type, _indexed?} -> argument == name && type end)
  end

  defp sample(:address, seed), do: <<seed::160>>
  defp sample({:bytes, 32}, seed), do: <<seed::256>>
  defp sample({:uint, bits}, seed) when bits <= 16, do: rem(seed, 200) + 1
  defp sample({:uint, _bits}, seed), do: seed * 7 + 3
  defp sample(:bool, seed), do: rem(seed, 2) == 0
  defp sample(:string, seed), do: "denom-#{seed}"

  defp expected(:address, value), do: "0x" <> Base.encode16(value, case: :lower)
  defp expected({:bytes, 32}, value), do: "0x" <> Base.encode16(value, case: :lower)
  defp expected(_type, value), do: value

  defp log(definition, index, values) do
    {indexed, unindexed} = Enum.split_with(definition.arguments, fn {_name, _type, indexed?} -> indexed? end)

    [second_topic, third_topic, fourth_topic] =
      indexed
      |> Enum.map(fn {name, type, _indexed?} -> topic(type, Map.fetch!(values, name)) end)
      |> Kernel.++([nil, nil, nil])
      |> Enum.take(3)

    %{
      address_hash: definition.address,
      block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
      block_number: 4_000_000 + index,
      data: data(unindexed, values),
      first_topic: definition.topic,
      second_topic: second_topic,
      third_topic: third_topic,
      fourth_topic: fourth_topic,
      index: index,
      transaction_hash: "0x8425a9b81a9bd1c64861110c1a453b84719cb0361d6fa0db68abf7611b9a890e"
    }
  end

  defp data([], _values), do: "0x"

  defp data(unindexed, values) do
    types = Enum.map(unindexed, fn {_name, type, _indexed?} -> type end)
    arguments = Enum.map(unindexed, fn {name, _type, _indexed?} -> Map.fetch!(values, name) end)

    "0x" <> Base.encode16(TypeEncoder.encode(arguments, types), case: :lower)
  end

  defp topic(:address, value), do: "0x" <> String.duplicate("0", 24) <> Base.encode16(value, case: :lower)
  defp topic({:bytes, 32}, value), do: "0x" <> Base.encode16(value, case: :lower)
  defp topic(:bool, value), do: topic({:uint, 256}, if(value, do: 1, else: 0))

  defp topic({:uint, _bits}, value) do
    "0x" <> (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(64, "0"))
  end
end
