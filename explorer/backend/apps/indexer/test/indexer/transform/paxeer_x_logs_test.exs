defmodule Indexer.Transform.PaxeerXLogsTest do
  use ExUnit.Case, async: true

  alias ABI.TypeEncoder
  alias Explorer.Chain.PaxeerX.{AccountBinding, Anchor, CustodyEvent, MarketEvent, Receipt}
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
    lx_anchors: Anchor,
    lx_market_events: MarketEvent,
    lx_receipts: Receipt
  }

  @empty %{
    lx_account_bindings: [],
    lx_custody_events: [],
    lx_anchors: [],
    lx_receipts: [],
    lx_market_events: []
  }

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
      assert by_table |> Map.fetch!(:lx_anchors) |> Enum.uniq() == [@anchor_precompile]

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

      rows = Enum.flat_map([:lx_account_bindings, :lx_custody_events, :lx_anchors, :lx_market_events], &result[&1])
      assert length(rows) == length(fixtures)

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

    test "an availability attestation is not decoded: it carries no checkpoint id" do
      log = %{
        address_hash: @anchor_precompile,
        block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
        block_number: 42,
        data: "0x" <> Base.encode16(TypeEncoder.encode([3, 1], [{:uint, 8}, {:uint, 8}]), case: :lower),
        first_topic:
          "0x" <> Base.encode16(ExKeccak.hash_256("AvailabilityAttested(uint64,bytes32,uint8,uint8)"), case: :lower),
        second_topic: "0x" <> String.duplicate("0", 59) <> "16463",
        third_topic: bytes32(0x5E),
        fourth_topic: nil,
        index: 0,
        transaction_hash: "0x43dfd761974e8c3351d285ab65bee311454eb45b149a015fe7804a33252f19e5"
      }

      assert PaxeerXLogs.parse([log]) == @empty
    end

    test "a deposit root registration is not decoded: it is no custody movement" do
      log = %{
        address_hash: @custody_precompile,
        block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
        block_number: 42,
        data:
          "0x" <>
            Base.encode16(TypeEncoder.encode([<<0x11::256>>, 1], [{:bytes, 32}, {:uint, 16}]), case: :lower),
        first_topic:
          "0x" <>
            Base.encode16(ExKeccak.hash_256("DepositRootRegistered(bytes32,bytes32,bytes32,uint16)"), case: :lower),
        second_topic: bytes32(0x21),
        third_topic: bytes32(0x22),
        fourth_topic: nil,
        index: 0,
        transaction_hash: "0x43dfd761974e8c3351d285ab65bee311454eb45b149a015fe7804a33252f19e5"
      }

      assert PaxeerXLogs.parse([log]) == @empty
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

    test "an unknown topic0 at a precompile address yields nothing" do
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

      assert PaxeerXLogs.parse([log]) == @empty
    end

    test "a guarantor event of the anchor precompile is left to its own lane" do
      log = %{
        address_hash: @anchor_precompile,
        block_hash: "0x79594150677f083756a37eee7b97ed99ab071f502104332cb3835bac345711ca",
        block_number: 42,
        data:
          "0x" <>
            Base.encode16(TypeEncoder.encode([<<1::160>>, 5, 1], [:address, {:uint, 256}, {:uint, 8}]), case: :lower),
        first_topic:
          "0x" <>
            Base.encode16(ExKeccak.hash_256("GuarantorRegistered(bytes32,address,address,uint256,uint8)"), case: :lower),
        second_topic: bytes32(1),
        third_topic: "0x" <> String.duplicate("0", 38) <> "02",
        fourth_topic: nil,
        index: 0,
        transaction_hash: "0x43dfd761974e8c3351d285ab65bee311454eb45b149a015fe7804a33252f19e5"
      }

      assert PaxeerXLogs.parse([log]) == @empty
    end

    test "a log missing an indexed topic is skipped rather than imported" do
      definition = definition!("ClaimFinalised")

      log =
        definition
        |> log(2, %{"claimId" => <<0x01::256>>, "nullifier" => <<0x02::256>>})
        |> Map.put(:third_topic, nil)

      assert PaxeerXLogs.parse([log]) == @empty
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
