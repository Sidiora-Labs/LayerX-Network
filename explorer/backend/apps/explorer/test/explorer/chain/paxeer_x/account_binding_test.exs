defmodule Explorer.Chain.PaxeerX.AccountBindingTest do
  use Explorer.DataCase

  alias Ecto.Multi
  alias Explorer.Chain.Import.Runner.PaxeerX.AccountBindings
  alias Explorer.Chain.PaxeerX.AccountBinding

  describe "changeset/2" do
    test "accepts the four identity spellings of a bound account" do
      %{block: block, transaction: transaction} = block_with_transaction()

      changeset = AccountBinding.changeset(%AccountBinding{}, attributes(block, transaction))

      assert changeset.valid?
      assert {:ok, binding} = Repo.insert(changeset)
      assert binding.bound
      assert binding.pax_address == "pax1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"
      assert String.starts_with?(binding.layerx_did, "did:layerx:")
      assert String.starts_with?(binding.layerx_account, "agent:did:layerx:")
      assert binding.block_number == block.number
      assert binding.transaction_hash == transaction.hash
    end

    test "requires the key, the address and the bound flag" do
      changeset = AccountBinding.changeset(%AccountBinding{}, %{})

      refute changeset.valid?

      errors = changeset_errors(changeset)

      assert errors[:transaction_hash] == ["can't be blank"]
      assert errors[:log_index] == ["can't be blank"]
      assert errors[:block_hash] == ["can't be blank"]
      assert errors[:block_number] == ["can't be blank"]
      assert errors[:evm_address_hash] == ["can't be blank"]
      assert errors[:bound] == ["can't be blank"]
    end

    test "accepts an unbinding that carries no kernel identity yet" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs =
        block
        |> attributes(transaction)
        |> Map.merge(%{bound: false, pax_address: nil, layerx_did: nil, layerx_account: nil})

      assert {:ok, binding} = %AccountBinding{} |> AccountBinding.changeset(attrs) |> Repo.insert()
      refute binding.bound
      assert is_nil(binding.layerx_did)
    end

    test "rejects a binding whose transaction is not indexed" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = block |> attributes(transaction) |> Map.put(:transaction_hash, transaction_hash())

      assert {:error, changeset} = %AccountBinding{} |> AccountBinding.changeset(attrs) |> Repo.insert()
      assert changeset_errors(changeset)[:transaction_hash] == ["does not exist"]
    end
  end

  describe "run/3" do
    test "inserts a binding row keyed by transaction hash and log index" do
      %{block: block, transaction: transaction} = block_with_transaction()

      assert {:ok, %{insert_paxeer_x_account_bindings: [inserted]}} =
               run_changes([attributes(block, transaction)])

      assert inserted.transaction_hash == transaction.hash
      assert inserted.log_index == 3
      assert inserted.block_consensus

      assert [stored] = Repo.all(AccountBinding)
      assert stored.transaction_hash == transaction.hash
      assert stored.layerx_account == inserted.layerx_account
    end

    test "leaves the stored row untouched when the same log is imported again" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_account_bindings: [_]}} = run_changes([attrs])

      assert {:ok, %{insert_paxeer_x_account_bindings: []}} =
               run_changes([Map.merge(attrs, %{bound: false, layerx_did: nil})])

      assert [stored] = Repo.all(AccountBinding)
      assert stored.bound
      assert String.starts_with?(stored.layerx_did, "did:layerx:")
    end

    test "keeps the rows of two logs of the same transaction apart" do
      %{block: block, transaction: transaction} = block_with_transaction()

      attrs = attributes(block, transaction)

      assert {:ok, %{insert_paxeer_x_account_bindings: inserted}} =
               run_changes([attrs, Map.merge(attrs, %{log_index: 4, bound: false})])

      assert length(inserted) == 2
      assert Repo.aggregate(AccountBinding, :count) == 2
    end

    test "handles an empty changes list" do
      assert {:ok, %{insert_paxeer_x_account_bindings: []}} = run_changes([])
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

      assert [kept] = Repo.all(AccountBinding.only_consensus_query())
      assert kept.transaction_hash == kept_transaction.hash

      {1, nil} =
        [reorged_block.hash]
        |> AccountBinding.lose_consensus_query()
        |> Repo.update_all(set: [block_consensus: false])

      assert [false] =
               AccountBinding
               |> where([binding], binding.block_hash == ^reorged_block.hash)
               |> select([binding], binding.block_consensus)
               |> Repo.all()
    end
  end

  defp block_with_transaction do
    block = insert(:block)
    transaction = :transaction |> insert() |> with_block(block)

    %{block: block, transaction: transaction}
  end

  defp attributes(block, transaction) do
    did_key = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

    %{
      transaction_hash: transaction.hash,
      log_index: 3,
      block_hash: block.hash,
      block_number: block.number,
      evm_address_hash: address_hash(),
      pax_address: "pax1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
      layerx_did: "did:layerx:" <> did_key,
      layerx_account: "agent:did:layerx:" <> did_key <> ":main",
      bound: true,
      nonce: 7
    }
  end

  defp run_changes(changes) when is_list(changes) do
    Multi.new()
    |> AccountBindings.run(changes, %{
      timeout: :infinity,
      timestamps: %{inserted_at: DateTime.utc_now(), updated_at: DateTime.utc_now()}
    })
    |> Repo.transaction()
  end
end
