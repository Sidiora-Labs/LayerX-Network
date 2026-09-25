defmodule Explorer.Chain.PaxeerX.ImportTest do
  use Explorer.DataCase

  alias Explorer.Chain.Import
  alias Explorer.Chain.PaxeerX.{AccountBinding, Anchor, CustodyEvent, DepositRoot, GuarantorEvent, MarketEvent, Receipt}

  setup do
    chain_type = Application.get_env(:explorer, :chain_type)
    Application.put_env(:explorer, :chain_type, :paxeer_x)

    on_exit(fn -> Application.put_env(:explorer, :chain_type, chain_type) end)

    block = insert(:block)
    transaction = :transaction |> insert() |> with_block(block)

    %{block: block, transaction: transaction}
  end

  describe "Explorer.Chain.Import.all/1 with the paxeer_x runners" do
    test "writes one row per lx_* table and reads it back", %{block: block, transaction: transaction} do
      assert {:ok, imported} = Import.all(import_data(block, transaction))

      assert [%AccountBinding{}] = imported[:insert_paxeer_x_account_bindings]
      assert [%CustodyEvent{}] = imported[:insert_paxeer_x_custody_events]
      assert [%Anchor{}] = imported[:insert_paxeer_x_anchors]
      assert [%GuarantorEvent{}] = imported[:insert_paxeer_x_guarantor_events]
      assert [%DepositRoot{}] = imported[:insert_paxeer_x_deposit_roots]
      assert [%Receipt{}] = imported[:insert_paxeer_x_receipts]
      assert [%MarketEvent{}] = imported[:insert_paxeer_x_market_events]

      binding = Repo.one!(AccountBinding)
      assert binding.transaction_hash == transaction.hash
      assert binding.block_hash == block.hash
      assert binding.block_number == block.number
      assert binding.log_index == 0
      assert binding.bound == true
      assert binding.event_name == "LayerXBound"
      assert to_string(binding.evm_address_hash) == "0x0000000000000000000000000000000000000011"
      assert binding.layerx_did == "did:layerx:" <> String.duplicate("0", 62) <> "22"
      assert binding.nonce == 7
      assert binding.parameters["nonce"] == 7
      assert binding.block_consensus == true

      custody = Repo.one!(CustodyEvent)
      assert custody.kind == :custody_deposit
      assert custody.direction == :deposit
      assert custody.event_name == "CustodyDeposit"
      assert to_string(custody.reference_id) == bytes32(0xAB)
      assert to_string(custody.asset_id) == bytes32(0xCD)
      assert to_string(custody.address_hash) == "0x00000000000000000000000000000000000000ef"
      assert to_string(custody.account) == bytes32(0x99)
      assert Decimal.equal?(custody.amount, Decimal.new(1_500_000_000_000_000_000))
      assert to_string(custody.nullifier) == bytes32(0x07)
      assert to_string(custody.checkpoint_hash) == bytes32(0x08)
      assert custody.parameters["amount"] == 1_500_000_000_000_000_000

      anchor = Repo.one!(Anchor)
      assert anchor.status == :submitted
      assert anchor.event_name == "CheckpointSubmitted"
      assert anchor.batch_number == 91_234
      assert to_string(anchor.checkpoint_id) == bytes32(0x7A)
      assert to_string(anchor.state_root) == bytes32(0x7B)
      assert to_string(anchor.receipt_root) == bytes32(0x7C)
      assert anchor.signers == 9
      assert anchor.parameters["batchNumber"] == 91_234

      guarantor = Repo.one!(GuarantorEvent)
      assert guarantor.kind == :guarantor_slashed
      assert guarantor.event_name == "GuarantorSlashed"
      assert to_string(guarantor.guarantor_id) == bytes32(0x1A)
      assert guarantor.batch_number == 91_234
      assert Decimal.equal?(guarantor.amount, Decimal.new(4_000_000_000_000_000_000))
      assert to_string(guarantor.reporter_address_hash) == "0x0000000000000000000000000000000000000031"
      assert Decimal.equal?(guarantor.reporter_reward, Decimal.new(400_000_000_000_000_000))
      assert guarantor.parameters["reason"] == 2

      deposit_root = Repo.one!(DepositRoot)
      assert deposit_root.event_name == "DepositRootRegistered"
      assert to_string(deposit_root.checkpoint_id) == bytes32(0x6A)
      assert to_string(deposit_root.deposit_root) == bytes32(0x6B)
      assert to_string(deposit_root.commitment) == bytes32(0x6C)
      assert deposit_root.version == 1
      assert deposit_root.parameters["version"] == 1

      receipt = Repo.one!(Receipt)
      assert receipt.status == :checkpoint_finalised
      assert to_string(receipt.receipt_id) == bytes32(0x5A)
      assert to_string(receipt.account) == bytes32(0x5B)

      market = Repo.one!(MarketEvent)
      assert market.domain == :launchpad
      assert market.kind == "swap"
      assert market.event_name == "Swap"
      assert to_string(market.address_hash) == "0x0000000000000000000000000000000000000052"
      assert to_string(market.asset_address_hash) == "0x0000000000000000000000000000000000000051"
      assert market.parameters["isBuy"] == true
    end

    test "re-importing the same logs keeps one row per log", %{block: block, transaction: transaction} do
      data = import_data(block, transaction)

      assert {:ok, _} = Import.all(data)
      assert {:ok, _} = Import.all(data)

      assert Repo.aggregate(AccountBinding, :count) == 1
      assert Repo.aggregate(CustodyEvent, :count) == 1
      assert Repo.aggregate(Anchor, :count) == 1
      assert Repo.aggregate(GuarantorEvent, :count) == 1
      assert Repo.aggregate(DepositRoot, :count) == 1
      assert Repo.aggregate(Receipt, :count) == 1
      assert Repo.aggregate(MarketEvent, :count) == 1
    end

    test "a row whose enum the schema does not declare is rejected before any insert", %{
      block: block,
      transaction: transaction
    } do
      data =
        block
        |> import_data(transaction)
        |> put_in([:paxeer_x_custody_events, :params, Access.at(0), :kind], :custody_rehypothecation)

      assert {:error, [changeset]} = Import.all(data)
      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:kind]
      assert Repo.aggregate(CustodyEvent, :count) == 0
      assert Repo.aggregate(AccountBinding, :count) == 0
    end

    test "a guarantor event whose kind the schema does not declare is rejected before any insert", %{
      block: block,
      transaction: transaction
    } do
      data =
        block
        |> import_data(transaction)
        |> put_in([:paxeer_x_guarantor_events, :params, Access.at(0), :kind], :guarantor_retired)

      assert {:error, [changeset]} = Import.all(data)
      refute changeset.valid?
      assert {"is invalid", _} = changeset.errors[:kind]
      assert Repo.aggregate(GuarantorEvent, :count) == 0
      assert Repo.aggregate(DepositRoot, :count) == 0
    end
  end

  defp import_data(block, transaction) do
    key = %{
      transaction_hash: transaction.hash,
      block_hash: block.hash,
      block_number: block.number
    }

    %{
      paxeer_x_account_bindings: %{
        params: [
          Map.merge(key, %{
            log_index: 0,
            event_name: "LayerXBound",
            parameters: %{"evm" => address(0x11), "didPublicKey" => bytes32(0x22), "nonce" => 7},
            bound: true,
            evm_address_hash: address(0x11),
            layerx_did: "did:layerx:" <> String.duplicate("0", 62) <> "22",
            nonce: 7
          })
        ]
      },
      paxeer_x_custody_events: %{
        params: [
          Map.merge(key, %{
            log_index: 1,
            event_name: "CustodyDeposit",
            parameters: %{
              "depositId" => bytes32(0xAB),
              "assetId" => bytes32(0xCD),
              "payer" => address(0xEF),
              "beneficiary" => bytes32(0x99),
              "amount" => 1_500_000_000_000_000_000,
              "nonce" => 42
            },
            kind: :custody_deposit,
            direction: :deposit,
            reference_id: bytes32(0xAB),
            nullifier: bytes32(0x07),
            checkpoint_hash: bytes32(0x08),
            asset_id: bytes32(0xCD),
            amount: 1_500_000_000_000_000_000,
            address_hash: address(0xEF),
            account: bytes32(0x99)
          })
        ]
      },
      paxeer_x_anchors: %{
        params: [
          Map.merge(key, %{
            log_index: 2,
            event_name: "CheckpointSubmitted",
            parameters: %{
              "batchNumber" => 91_234,
              "checkpointId" => bytes32(0x7A),
              "stateRoot" => bytes32(0x7B),
              "receiptRoot" => bytes32(0x7C),
              "signers" => 9
            },
            status: :submitted,
            batch_number: 91_234,
            checkpoint_id: bytes32(0x7A),
            state_root: bytes32(0x7B),
            receipt_root: bytes32(0x7C),
            signers: 9
          })
        ]
      },
      paxeer_x_guarantor_events: %{
        params: [
          Map.merge(key, %{
            log_index: 5,
            event_name: "GuarantorSlashed",
            parameters: %{
              "guarantorId" => bytes32(0x1A),
              "reason" => 2,
              "batchNumber" => 91_234,
              "amount" => 4_000_000_000_000_000_000,
              "reporter" => address(0x31),
              "reporterReward" => 400_000_000_000_000_000
            },
            kind: :guarantor_slashed,
            guarantor_id: bytes32(0x1A),
            batch_number: 91_234,
            amount: 4_000_000_000_000_000_000,
            reporter_address_hash: address(0x31),
            reporter_reward: 400_000_000_000_000_000
          })
        ]
      },
      paxeer_x_deposit_roots: %{
        params: [
          Map.merge(key, %{
            log_index: 6,
            event_name: "DepositRootRegistered",
            parameters: %{
              "checkpointId" => bytes32(0x6A),
              "depositRoot" => bytes32(0x6B),
              "commitment" => bytes32(0x6C),
              "version" => 1
            },
            checkpoint_id: bytes32(0x6A),
            deposit_root: bytes32(0x6B),
            commitment: bytes32(0x6C),
            version: 1
          })
        ]
      },
      paxeer_x_receipts: %{
        params: [
          Map.merge(key, %{
            log_index: 3,
            receipt_id: bytes32(0x5A),
            account: bytes32(0x5B),
            payload_hash: bytes32(0x5C),
            status: :checkpoint_finalised
          })
        ]
      },
      paxeer_x_market_events: %{
        params: [
          Map.merge(key, %{
            log_index: 4,
            event_name: "Swap",
            parameters: %{
              "token" => address(0x51),
              "trader" => address(0x52),
              "recipient" => address(0x53),
              "isBuy" => true,
              "amountIn" => 100,
              "amountOut" => 98,
              "feeAmount" => 2,
              "price" => 1_020_000
            },
            domain: :launchpad,
            kind: "swap",
            address_hash: address(0x52),
            asset_address_hash: address(0x51)
          })
        ]
      }
    }
  end

  defp bytes32(value), do: "0x" <> (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(64, "0"))

  defp address(value), do: "0x" <> (value |> Integer.to_string(16) |> String.downcase() |> String.pad_leading(40, "0"))
end
