defmodule BlockScoutWeb.API.V2.PaxeerX.OpenApiSpecTest do
  use BlockScoutWeb.ConnCase

  alias Explorer.Chain.Address
  alias Explorer.Chain.PaxeerX.{AccountBinding, Anchor, CustodyEvent, Receipt}
  alias Explorer.Repo
  alias OpenApiSpex.{Cast, MediaType, Operation, PathItem, Response, Schema}

  @pax_address "pax1005qwm6w5jj26zq8tsjs3eyp6my5d5fthrlqk3"
  @kernel_key "3f1a9b0c5d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8"
  @asset_id "0x" <> String.duplicate("0", 63) <> "1"

  @paxeer_x_paths [
    "/v2/addresses/{address_hash_param}/unified",
    "/v2/paxeer-x/anchors",
    "/v2/paxeer-x/capabilities",
    "/v2/paxeer-x/receipts",
    "/v2/paxeer-x/receipts/{id}",
    "/v2/transactions/{transaction_hash_param}/status"
  ]

  describe "the generated specification" do
    test "lists the six Paxeer X paths with no registration beyond the API router" do
      spec = BlockScoutWeb.Specs.Public.spec()

      for path <- @paxeer_x_paths do
        assert %PathItem{get: %Operation{} = operation} = Map.get(spec.paths, path),
               "the specification carries no GET operation for #{path}"

        assert operation.summary
        assert "paxeer-x" in operation.tags
        assert Enum.any?(operation.parameters, &(&1.name == :apikey))
        assert %Response{content: %{"application/json" => %MediaType{}}} = success_response(operation)
      end
    end
  end

  describe "the declared response schemas" do
    test "reject a unified account document that lost a field", %{conn: conn} do
      address = insert(:address)
      block = insert(:block, number: 300)
      transaction = :transaction |> insert() |> with_block(block)

      insert_binding(address, block, transaction)

      insert_custody_event(block, transaction,
        log_index: 1,
        kind: :custody_deposit,
        direction: :deposit,
        amount: Decimal.new(42),
        address_hash: to_string(address.hash),
        account: "0x" <> @kernel_key
      )

      path = "/api/v2/addresses/#{Address.checksum(address.hash)}/unified"
      response = json_response(get(conn, path), 200)

      refute Enum.empty?(response["balances"])
      refute Enum.empty?(response["activity"])

      {spec, schema} = declared_schema("/v2/addresses/{address_hash_param}/unified")

      assert_absent_rejected(response, schema, spec)
      assert_absent_rejected_in_list(response, "balances", schema, spec)
      assert_absent_rejected_in_list(response, "activity", schema, spec)
    end

    test "reject an anchors page that lost a field", %{conn: conn} do
      insert_anchor(3)

      response = json_response(get(conn, "/api/v2/paxeer-x/anchors"), 200)
      {spec, schema} = declared_schema("/v2/paxeer-x/anchors")

      assert_absent_rejected(response, schema, spec)
      assert_absent_rejected_in_list(response, "items", schema, spec)
    end

    test "reject a capabilities document that lost a field", %{conn: conn} do
      response = json_response(get(conn, "/api/v2/paxeer-x/capabilities"), 200)
      {spec, schema} = declared_schema("/v2/paxeer-x/capabilities")

      assert_absent_rejected(response, schema, spec)
    end

    test "reject a receipts page that lost a field", %{conn: conn} do
      insert_receipt(4)

      response = json_response(get(conn, "/api/v2/paxeer-x/receipts"), 200)
      {spec, schema} = declared_schema("/v2/paxeer-x/receipts")

      assert_absent_rejected(response, schema, spec)
      assert_absent_rejected_in_list(response, "items", schema, spec)
    end

    test "reject one receipt that lost a field", %{conn: conn} do
      insert_receipt(4)

      response = json_response(get(conn, "/api/v2/paxeer-x/receipts/#{receipt_id(4)}"), 200)
      {spec, schema} = declared_schema("/v2/paxeer-x/receipts/{id}")

      assert_absent_rejected(response, schema, spec)
    end

    test "reject a transaction status that lost a field", %{conn: conn} do
      block = insert(:block, number: 4_242)
      transaction = :transaction |> insert() |> with_block(block)

      response = json_response(get(conn, "/api/v2/transactions/#{transaction.hash}/status"), 200)
      {spec, schema} = declared_schema("/v2/transactions/{transaction_hash_param}/status")

      assert_absent_rejected(response, schema, spec)
    end
  end

  defp declared_schema(path) do
    spec = BlockScoutWeb.Specs.Public.spec()

    %PathItem{get: %Operation{} = operation} = Map.fetch!(spec.paths, path)

    %Response{content: %{"application/json" => %MediaType{schema: schema}}} = success_response(operation)

    {spec, resolve(schema, spec)}
  end

  defp success_response(%Operation{responses: responses}),
    do: Map.get(responses, 200) || Map.get(responses, "200")

  defp resolve(schema, spec), do: OpenApiSpex.resolve_schema(schema, spec.components.schemas)

  defp assert_absent_rejected(document, %Schema{} = schema, spec) do
    assert {:ok, _document} = cast(document, schema, spec)

    for field <- required_fields(schema) do
      assert {:error, _errors} = cast(Map.delete(document, field), schema, spec),
             "the declared schema accepts a document without #{field}"
    end
  end

  defp assert_absent_rejected_in_list(document, key, %Schema{} = schema, spec) do
    item_schema = schema.properties |> Map.fetch!(String.to_existing_atom(key)) |> Map.fetch!(:items) |> resolve(spec)

    [item | rest] = Map.fetch!(document, key)

    for field <- required_fields(item_schema) do
      broken = Map.put(document, key, [Map.delete(item, field) | rest])

      assert {:error, _errors} = cast(broken, schema, spec),
             "the declared schema accepts a #{key} item without #{field}"
    end
  end

  defp required_fields(%Schema{required: required}) when is_list(required),
    do: Enum.map(required, &to_string/1)

  defp cast(document, %Schema{} = schema, spec),
    do: Cast.cast(%Cast{value: document, schema: schema, schemas: spec.components.schemas})

  defp insert_binding(address, block, transaction) do
    did = "did:layerx:" <> @kernel_key

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: 0,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      evm_address_hash: to_string(address.hash),
      pax_address: @pax_address,
      layerx_did: did,
      layerx_account: "agent:" <> did <> ":main",
      bound: true,
      nonce: 1
    }

    %AccountBinding{}
    |> AccountBinding.changeset(attributes)
    |> Repo.insert!()
  end

  defp insert_custody_event(block, transaction, fields) do
    attributes =
      Map.merge(
        %{
          transaction_hash: transaction.hash,
          block_hash: block.hash,
          block_number: block.number,
          block_consensus: true,
          asset_id: @asset_id
        },
        Map.new(fields)
      )

    %CustodyEvent{}
    |> CustodyEvent.changeset(attributes)
    |> Repo.insert!()
  end

  defp insert_anchor(batch_number) do
    block = insert(:block, number: batch_number * 10)
    transaction = :transaction |> insert() |> with_block(block)

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: 0,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      batch_number: batch_number,
      checkpoint_id: "0x" <> Base.encode16(<<batch_number::256>>, case: :lower),
      state_root: to_string(block_hash()),
      receipt_root: to_string(block_hash()),
      signers: 5,
      status: :submitted,
      kernel_height: batch_number * 100,
      sealed_height: batch_number * 100 - 1
    }

    %Anchor{}
    |> Anchor.changeset(attributes)
    |> Repo.insert!()
  end

  defp insert_receipt(index) do
    block = insert(:block, number: index * 10)
    transaction = :transaction |> insert() |> with_block(block)

    attributes = %{
      transaction_hash: transaction.hash,
      log_index: 0,
      block_hash: block.hash,
      block_number: block.number,
      block_consensus: true,
      receipt_id: receipt_id(index),
      account: "0x" <> @kernel_key,
      payload_hash: to_string(block_hash()),
      status: :sequencer_signed
    }

    %Receipt{}
    |> Receipt.changeset(attributes)
    |> Repo.insert!()
  end

  defp receipt_id(index), do: "0x" <> Base.encode16(<<index::256>>, case: :lower)
end
