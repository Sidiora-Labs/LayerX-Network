defmodule BlockScoutWeb.API.V2.PaxeerX.IdentitySearchTest do
  use BlockScoutWeb.ConnCase

  alias Explorer.Chain.Address
  alias Explorer.Chain.PaxeerX.AccountBinding
  alias Explorer.Repo

  @pax_address "pax1005qwm6w5jj26zq8tsjs3eyp6my5d5fthrlqk3"
  @kernel_key "3f1a9b0c5d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8"

  setup do
    address = insert(:address)
    did = "did:layerx:" <> @kernel_key
    kernel_account = "agent:" <> did <> ":main"

    insert_binding(address, did, kernel_account, true, 0)

    {:ok, address: address, did: did, kernel_account: kernel_account}
  end

  describe "GET /api/v2/search" do
    test "a pax bech32 string finds the account it is bound to", %{conn: conn, address: address} do
      response = json_response(get(conn, "/api/v2/search", %{"q" => @pax_address}), 200)

      assert [%{"type" => "address", "address_hash" => address_hash}] = response["items"]
      assert address_hash == Address.checksum(address.hash)
    end

    test "a DID finds the account it is bound to", %{conn: conn, address: address, did: did} do
      response = json_response(get(conn, "/api/v2/search", %{"q" => did}), 200)

      assert [%{"type" => "address", "address_hash" => address_hash}] = response["items"]
      assert address_hash == Address.checksum(address.hash)
    end

    test "a kernel account id finds the account it is bound to", %{
      conn: conn,
      address: address,
      kernel_account: kernel_account
    } do
      response = json_response(get(conn, "/api/v2/search", %{"q" => kernel_account}), 200)

      assert [%{"type" => "address", "address_hash" => address_hash}] = response["items"]
      assert address_hash == Address.checksum(address.hash)
    end

    test "a bare kernel key finds the account it is bound to", %{conn: conn, address: address} do
      response = json_response(get(conn, "/api/v2/search", %{"q" => @kernel_key}), 200)

      assert [%{"type" => "address", "address_hash" => address_hash}] = response["items"]
      assert address_hash == Address.checksum(address.hash)
    end

    test "an unbound DID finds nothing", %{conn: conn} do
      unbound = "did:layerx:" <> String.duplicate("ab", 32)

      response = json_response(get(conn, "/api/v2/search", %{"q" => unbound}), 200)

      assert response["items"] == []
    end

    test "an identity the newest log unbound finds nothing", %{conn: conn, address: address, did: did} do
      kernel_account = "agent:" <> did <> ":main"

      insert_binding(address, did, kernel_account, false, 1)

      response = json_response(get(conn, "/api/v2/search", %{"q" => did}), 200)

      assert response["items"] == []
    end
  end

  defp insert_binding(address, did, kernel_account, bound, log_index) do
    block = insert(:block, number: 10 + log_index)
    transaction = :transaction |> insert() |> with_block(block)

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: log_index,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      evm_address_hash: to_string(address.hash),
      pax_address: @pax_address,
      layerx_did: did,
      layerx_account: kernel_account,
      bound: bound,
      nonce: log_index
    }

    %AccountBinding{}
    |> AccountBinding.changeset(attributes)
    |> Repo.insert!()
  end
end
