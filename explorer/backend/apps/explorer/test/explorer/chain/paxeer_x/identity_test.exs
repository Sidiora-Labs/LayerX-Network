defmodule Explorer.Chain.PaxeerX.IdentityTest do
  use ExUnit.Case, async: true

  alias Explorer.Chain.PaxeerX.Identity

  @evm_address "0x7be8076f4ea4a4ad08075c2508e481d6c946d12b"
  @pax_address "pax1005qwm6w5jj26zq8tsjs3eyp6my5d5fthrlqk3"
  @kernel_key "3f1a9b0c5d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8"

  describe "parse/1" do
    test "reads an EVM address with and without the prefix" do
      assert {:ok, %Identity{kind: :evm, evm: hash}} = Identity.parse(@evm_address)
      assert to_string(hash) == @evm_address

      assert {:ok, %Identity{kind: :evm, evm: ^hash}} = Identity.parse(String.trim_leading(@evm_address, "0x"))
    end

    test "reads a pax bech32 string and rejects a broken checksum" do
      assert {:ok, %Identity{kind: :pax, pax: @pax_address}} = Identity.parse(@pax_address)

      assert :error = Identity.parse(String.replace_suffix(@pax_address, "3", "4"))
      assert :error = Identity.parse(String.replace_prefix(@pax_address, "pax1", "cosmos1"))
    end

    test "reads a DID and derives the kernel account id from it" do
      assert {:ok, %Identity{kind: :did, did: did, kernel_account: kernel_account}} =
               Identity.parse("did:layerx:" <> @kernel_key)

      assert did == "did:layerx:" <> @kernel_key
      assert kernel_account == "agent:did:layerx:" <> @kernel_key <> ":main"
    end

    test "reads a kernel account id and derives the DID from it" do
      kernel_account = "agent:did:layerx:" <> @kernel_key <> ":main"

      assert {:ok, %Identity{kind: :kernel_account, did: did, kernel_account: ^kernel_account}} =
               Identity.parse(kernel_account)

      assert did == "did:layerx:" <> @kernel_key
    end

    test "reads a bare kernel key" do
      assert {:ok, %Identity{kind: :kernel_account, kernel_account: @kernel_key, did: nil}} =
               Identity.parse(@kernel_key)
    end

    test "rejects everything else" do
      assert :error = Identity.parse("")
      assert :error = Identity.parse("0x")
      assert :error = Identity.parse("did:layerx:notahexkey")
      assert :error = Identity.parse("agent:did:layerx:" <> @kernel_key)
      assert :error = Identity.parse(nil)
    end
  end

  describe "key/1" do
    test "reads the key out of every spelling that carries one" do
      assert Identity.key("did:layerx:" <> @kernel_key) == @kernel_key
      assert Identity.key("agent:did:layerx:" <> @kernel_key <> ":main") == @kernel_key
      assert Identity.key(@kernel_key) == @kernel_key
      assert Identity.key(@evm_address) == nil
      assert Identity.key(nil) == nil
    end
  end
end
