defmodule Explorer.Chain.PaxeerX.Identity do
  @moduledoc """
  One account identity in the four spellings the Paxeer X Network uses.

  The EVM chain spells an account as a 20-byte address, the association layer as a `pax`
  bech32 string, and the LayerX kernel as a `did:layerx:<key>` document or as the account id
  `agent:did:layerx:<key>:main` derived from it.

  `parse/1` accepts any of the four and fills in only what follows from the input itself: the
  kernel account id and the DID are two spellings of the same 32-byte key, so either one yields
  the other, while the EVM address and the `pax` string are bindings that have to be read from
  `lx_account_bindings` by `Explorer.Chain.PaxeerX.UnifiedAccount`.
  """

  import Bitwise

  alias Explorer.Chain.Hash

  @enforce_keys [:kind, :input]
  defstruct [:kind, :input, :evm, :pax, :did, :kernel_account]

  @typedoc """
  Which spelling the caller supplied.
  """
  @type kind :: :evm | :pax | :did | :kernel_account

  @type t :: %__MODULE__{
          kind: kind(),
          input: String.t(),
          evm: Hash.Address.t() | nil,
          pax: String.t() | nil,
          did: String.t() | nil,
          kernel_account: String.t() | nil
        }

  @did_prefix "did:layerx:"
  @kernel_account_prefix "agent:did:layerx:"
  @kernel_account_suffix ":main"
  @pax_hrp "pax"
  @key_length 64
  @address_length 40
  @bech32_charset ~c"qpzry9x8gf2tvdw0s3jn54khce6mua7l"
  @bech32_generators [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3]
  @bech32_checksum_length 6
  @bech32_max_length 90

  @doc """
  Parses an EVM address, a `pax` bech32 string, a `did:layerx:` DID or a kernel account id.

  Returns `:error` for anything else, including a bech32 string with another human readable
  part or a broken checksum.
  """
  @spec parse(term()) :: {:ok, t()} | :error
  def parse(input) when is_binary(input) do
    trimmed = String.trim(input)

    with :error <- parse_evm(trimmed),
         :error <- parse_did(trimmed),
         :error <- parse_kernel_account(trimmed),
         :error <- parse_pax(trimmed) do
      :error
    end
  end

  def parse(_input), do: :error

  @doc """
  The DID of a 32-byte kernel key, lower case hexadecimal without the `0x` prefix.
  """
  @spec did(String.t()) :: String.t()
  def did(key), do: @did_prefix <> key

  @doc """
  The kernel account id of a 32-byte kernel key, lower case hexadecimal without the `0x` prefix.
  """
  @spec kernel_account(String.t()) :: String.t()
  def kernel_account(key), do: @kernel_account_prefix <> key <> @kernel_account_suffix

  @doc """
  The bare key of a DID or of a kernel account id, or `nil` when the string carries no key.
  """
  @spec key(String.t() | nil) :: String.t() | nil
  def key(@did_prefix <> key), do: normalized_key(key)

  def key(@kernel_account_prefix <> rest) do
    if String.ends_with?(rest, @kernel_account_suffix) do
      rest
      |> binary_part(0, byte_size(rest) - byte_size(@kernel_account_suffix))
      |> normalized_key()
    end
  end

  def key(string) when is_binary(string), do: normalized_key(string)

  def key(nil), do: nil

  defp normalized_key(key) do
    case normalize_key(key) do
      {:ok, normalized} -> normalized
      :error -> nil
    end
  end

  defp parse_evm(input) do
    hex =
      case input do
        "0x" <> rest -> rest
        other -> other
      end

    with true <- byte_size(hex) == @address_length,
         true <- hex?(hex),
         {:ok, address_hash} <- Hash.Address.cast("0x" <> hex) do
      {:ok, %__MODULE__{kind: :evm, input: input, evm: address_hash}}
    else
      _ -> :error
    end
  end

  defp parse_did(@did_prefix <> key = input) do
    case normalize_key(key) do
      {:ok, key} ->
        {:ok, %__MODULE__{kind: :did, input: input, did: did(key), kernel_account: kernel_account(key)}}

      :error ->
        :error
    end
  end

  defp parse_did(_input), do: :error

  defp parse_kernel_account(@kernel_account_prefix <> rest = input) do
    with true <- String.ends_with?(rest, @kernel_account_suffix),
         key = binary_part(rest, 0, byte_size(rest) - byte_size(@kernel_account_suffix)),
         {:ok, key} <- normalize_key(key) do
      {:ok, %__MODULE__{kind: :kernel_account, input: input, did: did(key), kernel_account: kernel_account(key)}}
    else
      _ -> :error
    end
  end

  defp parse_kernel_account(input) when byte_size(input) == @key_length do
    case normalize_key(input) do
      {:ok, key} -> {:ok, %__MODULE__{kind: :kernel_account, input: input, kernel_account: key}}
      :error -> :error
    end
  end

  defp parse_kernel_account(_input), do: :error

  defp parse_pax(input) do
    case bech32_decode(input) do
      {:ok, @pax_hrp, _data} -> {:ok, %__MODULE__{kind: :pax, input: input, pax: String.downcase(input)}}
      _ -> :error
    end
  end

  defp normalize_key(key) do
    if byte_size(key) == @key_length and hex?(key) do
      {:ok, String.downcase(key)}
    else
      :error
    end
  end

  defp hex?(string), do: string |> String.to_charlist() |> Enum.all?(&hex_digit?/1)

  defp hex_digit?(character) do
    character in ?0..?9 or character in ?a..?f or character in ?A..?F
  end

  @spec bech32_decode(String.t()) :: {:ok, String.t(), binary()} | :error
  defp bech32_decode(string) when byte_size(string) <= @bech32_max_length do
    lower = String.downcase(string)

    if lower == string or String.upcase(string) == string do
      split_bech32(lower)
    else
      :error
    end
  end

  defp bech32_decode(_string), do: :error

  defp split_bech32(string) do
    case :binary.matches(string, "1") do
      [] ->
        :error

      matches ->
        {position, _length} = List.last(matches)

        decode_bech32(
          binary_part(string, 0, position),
          binary_part(string, position + 1, byte_size(string) - position - 1)
        )
    end
  end

  defp decode_bech32(human_readable_part, data) do
    with true <- human_readable_part != "",
         true <- human_readable_part |> String.to_charlist() |> Enum.all?(&(&1 in 33..126)),
         true <- byte_size(data) >= @bech32_checksum_length,
         {:ok, values} <- decode_charset(data),
         true <- polymod(hrp_expand(human_readable_part) ++ values) == 1,
         {:ok, bytes} <- convert_bits(Enum.drop(values, -@bech32_checksum_length), 5, 8, false) do
      {:ok, human_readable_part, bytes}
    else
      _ -> :error
    end
  end

  defp decode_charset(data) do
    data
    |> String.to_charlist()
    |> Enum.reduce_while({:ok, []}, fn character, {:ok, acc} ->
      case Enum.find_index(@bech32_charset, &(&1 == character)) do
        nil -> {:halt, :error}
        index -> {:cont, {:ok, [index | acc]}}
      end
    end)
    |> case do
      {:ok, reversed} -> {:ok, Enum.reverse(reversed)}
      :error -> :error
    end
  end

  defp hrp_expand(human_readable_part) do
    characters = String.to_charlist(human_readable_part)

    Enum.map(characters, &bsr(&1, 5)) ++ [0] ++ Enum.map(characters, &band(&1, 31))
  end

  defp polymod(values) do
    Enum.reduce(values, 1, fn value, checksum ->
      top = bsr(checksum, 25)
      folded = bxor(bsl(band(checksum, 0x1FFFFFF), 5), value)

      @bech32_generators
      |> Enum.with_index()
      |> Enum.reduce(folded, fn {generator, index}, acc ->
        if band(bsr(top, index), 1) == 1, do: bxor(acc, generator), else: acc
      end)
    end)
  end

  defp convert_bits(values, from, to, pad), do: convert_bits(values, from, to, pad, 0, 0, [])

  defp convert_bits([], _from, to, pad, accumulator, bits, output) do
    cond do
      pad and bits > 0 ->
        {:ok, :binary.list_to_bin(Enum.reverse([band(bsl(accumulator, to - bits), mask(to)) | output]))}

      bits >= to or band(accumulator, mask(bits)) != 0 ->
        :error

      true ->
        {:ok, :binary.list_to_bin(Enum.reverse(output))}
    end
  end

  defp convert_bits([value | rest], from, to, pad, accumulator, bits, output) do
    if value < 0 or bsr(value, from) != 0 do
      :error
    else
      {accumulator, bits, output} = emit(bor(bsl(accumulator, from), value), bits + from, to, output)

      convert_bits(rest, from, to, pad, accumulator, bits, output)
    end
  end

  defp emit(accumulator, bits, to, output) when bits >= to do
    remaining = bits - to

    emit(accumulator, remaining, to, [band(bsr(accumulator, remaining), mask(to)) | output])
  end

  defp emit(accumulator, bits, _to, output), do: {band(accumulator, mask(bits)), bits, output}

  defp mask(bits), do: bsl(1, bits) - 1
end
