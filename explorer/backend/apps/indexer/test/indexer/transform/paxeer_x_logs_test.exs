defmodule Indexer.Transform.PaxeerXLogsTest do
  use ExUnit.Case, async: true

  alias ABI.TypeEncoder
  alias Indexer.Transform.PaxeerXLogs

  @addr_precompile "0x0000000000000000000000000000000000001004"
  @custody_precompile "0x0000000000000000000000000000000000001013"
  @anchor_precompile "0x0000000000000000000000000000000000001014"
  @exchange_precompile "0x0000000000000000000000000000000000001015"
  @bridge_precompile "0x0000000000000000000000000000000000001016"
  @launchpad_precompile "0x0000000000000000000000000000000000001017"

  @erc20_transfer_topic "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"

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
  end

  describe "parse/1" do
    test "decodes one ABI encoded log per precompile event into the row of its table" do
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
        assert row.event == definition.event
        assert row.address_hash == definition.address
        assert row.transaction_hash == log.transaction_hash
        assert row.block_hash == log.block_hash
        assert row.block_number == log.block_number
        assert row.parameters == arguments
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
      assert row.did_public_key == "0x0000000000000000000000000000000000000000000000000000000000000022"
      assert row.nonce == 7
      assert row.bound == true
      assert row.event == "bound"
      assert row.log_index == 12
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
      assert row.event == "unbound"
    end

    test "a custody deposit log fills the promoted custody columns" do
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

      assert row.event == "custody-deposit"
      assert row.deposit_id == "0x00000000000000000000000000000000000000000000000000000000000000ab"
      assert row.asset_id == "0x00000000000000000000000000000000000000000000000000000000000000cd"
      assert row.party_address_hash == "0x00000000000000000000000000000000000000ef"
      assert row.account == "0x0000000000000000000000000000000000000000000000000000000000000099"
      assert row.amount == 1_500_000_000_000_000_000
      assert row.claim_id == nil
      assert row.nullifier == nil
      assert row.checkpoint_hash == nil
      assert row.parameters["nonce"] == 42
    end

    test "a claim queued log keeps the three indexed identifiers apart" do
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

      assert row.claim_id == "0x" <> String.duplicate("0", 62) <> "01"
      assert row.nullifier == "0x" <> String.duplicate("0", 62) <> "02"
      assert row.checkpoint_hash == "0x" <> String.duplicate("0", 62) <> "03"
      assert row.asset_id == "0x" <> String.duplicate("0", 62) <> "04"
      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "05"
      assert row.amount == 250
      assert row.parameters["availableAt"] == 1_700_000_000
    end

    test "a checkpoint log fills the promoted anchor columns" do
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

      assert row.event == "checkpoint-submitted"
      assert row.batch_number == 91_234
      assert row.checkpoint_id == "0x" <> String.duplicate("0", 62) <> "7a"
      assert row.state_root == "0x" <> String.duplicate("0", 62) <> "7b"
      assert row.receipt_root == "0x" <> String.duplicate("0", 62) <> "7c"
      assert row.signers == 9
      assert row.guarantor_id == nil
    end

    test "an availability attestation lands in the anchor table with its guarantor" do
      definition = definition!("AvailabilityAttested")

      log =
        log(definition, 1, %{
          "batchNumber" => 91_235,
          "guarantorId" => <<0x5E::256>>,
          "classMask" => 3,
          "availabilityMask" => 1
        })

      assert %{lx_anchors: [row]} = PaxeerXLogs.parse([log])

      assert row.event == "availability-attested"
      assert row.guarantor_id == "0x" <> String.duplicate("0", 62) <> "5e"
      assert row.state_root == nil
      assert row.parameters["classMask"] == 3
      assert row.parameters["availabilityMask"] == 1
    end

    test "an exchange log is a market event of the exchange module" do
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

      assert row.module == "exchange"
      assert row.event == "order-placed"
      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "23"
      assert row.nonce == 11
      assert row.amount == nil
      assert row.token_address_hash == nil
      assert row.parameters["marketId"] == "0x" <> String.duplicate("0", 62) <> "22"
      assert row.parameters["price"] == 42_000_000
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

      assert row.module == "bridge"
      assert row.event == "bridge-in"
      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "32"
      assert row.token_address_hash == "0x" <> String.duplicate("0", 38) <> "33"
      assert row.amount == 10_000
      assert row.parameters["chain"] == 125
      assert row.parameters["denom"] == "ulxbtc"
      assert row.parameters["logIndex"] == 4
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

      assert row.nonce == 512
      assert row.token_address_hash == "0x" <> String.duplicate("0", 38) <> "41"
      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "42"
      assert row.amount == 77
    end

    test "a launchpad swap prefers the trader over the recipient as the party" do
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

      assert row.module == "launchpad"
      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "52"
      assert row.token_address_hash == "0x" <> String.duplicate("0", 38) <> "51"
      assert row.parameters["isBuy"] == true
      assert row.parameters["amountOut"] == 98
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

      assert row.party_address_hash == "0x" <> String.duplicate("0", 38) <> "62"
      assert row.parameters["denom"] == "factory/pax1/demo"
      assert row.parameters["name"] == "Demo Market"
      assert row.parameters["symbol"] == "DEMO"
      assert row.parameters["feeStrategy"] == 2
    end

    test "a topic0 written in upper case still matches" do
      definition = definition!("FeesBurned")

      log =
        definition
        |> log(10, %{"token" => <<0x71::160>>, "amount" => 5})
        |> Map.update!(:first_topic, &String.upcase/1)

      assert %{lx_market_events: [row]} = PaxeerXLogs.parse([log])

      assert row.event == "fees-burned"
      assert row.amount == 5
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
        second_topic: "0x" <> String.duplicate("0", 62) <> "01",
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

  defp definition!(name), do: Enum.find(PaxeerXLogs.definitions(), &(&1.name == name))

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
