defmodule Explorer.Chain.PaxeerX.Status do
  @moduledoc """
  The Paxeer X Network settlement ladder: the one rule every surface reads a
  block's, transaction's or receipt's settlement status from.

  An item sits on exactly one rung. It is `:pending` while no consensus block
  holds it, either because it has no block number yet or because its block
  number is above the highest consensus block the explorer has; it is
  `:instant` once a consensus block does hold it; it is `:sealed` once its block
  number is at or below the height the latest anchor checkpoint seals, which is
  read either from the latest `lx_anchors` row or, when the anchor capability is
  live, from the anchor precompile itself; and it is `:final` once its block
  number is at or below the finality height, which is the height covered by the
  latest checkpoint the anchor reports as finalized, or, when the anchor reports
  none and `PAXEER_X_FINAL_CONFIRMATIONS` is set above its default of zero, the
  highest consensus block minus that confirmation depth. The rungs are tested
  from the top down so the highest one that holds wins, and a height that no
  source reports stays absent rather than standing in as a zero, which leaves
  its rung out of reach instead of promoting every item onto it.

  `of/2` is pure: it reads nothing and decides only from the block number and
  the `t:Explorer.Chain.PaxeerX.Finality.Heights.t/0` context that
  `Explorer.Chain.PaxeerX.Finality.heights/1` collects. It answers with this
  struct, which carries the rung, the rule that decided it and every height and
  source that went into the decision, so a surface can show why an item sits
  where it does without asking again.
  """

  alias Explorer.Chain.PaxeerX.Finality.{Anchor, Heights}

  @typedoc """
  The rung an item sits on.
  """
  @type rung :: :pending | :instant | :sealed | :final

  @typedoc """
  The rule that decided the rung.

  * `:no_block` - the item has no block number.
  * `:above_head` - its block number is above the highest consensus block.
  * `:in_consensus_block` - a consensus block holds it.
  * `:head_unknown` - it has a block number and no head to compare it against.
  * `:sealed_by_anchor` - it is at or below the height the latest anchor seals.
  * `:final_by_anchor` - it is at or below the anchor's finality height.
  * `:final_by_confirmations` - it is at or below the head minus the configured
    confirmation depth, the anchor having reported no finalized checkpoint.
  """
  @type reason ::
          :no_block
          | :above_head
          | :in_consensus_block
          | :head_unknown
          | :sealed_by_anchor
          | :final_by_anchor
          | :final_by_confirmations

  @type t :: %__MODULE__{
          rung: rung(),
          reason: reason(),
          block_number: non_neg_integer() | nil,
          instant_height: non_neg_integer() | nil,
          sealed_height: non_neg_integer() | nil,
          sealed_source: Heights.sealed_source(),
          final_height: non_neg_integer() | nil,
          final_source: Heights.final_source(),
          anchor: Anchor.t() | nil
        }

  @enforce_keys [:rung, :reason]
  defstruct [
    :rung,
    :reason,
    :block_number,
    :instant_height,
    :sealed_height,
    :final_height,
    :anchor,
    sealed_source: :no_anchor,
    final_source: :none
  ]

  @doc """
  The rung `block_number` sits on, given the heights in `heights`.

  Pure. `block_number` is `nil` for an item no block holds yet.
  """
  @spec of(non_neg_integer() | nil, Heights.t()) :: t()
  def of(block_number, %Heights{} = heights)
      when is_nil(block_number) or (is_integer(block_number) and block_number >= 0) do
    {rung, reason} = decide(block_number, heights)

    %__MODULE__{
      rung: rung,
      reason: reason,
      block_number: block_number,
      instant_height: heights.instant_height,
      sealed_height: heights.sealed_height,
      sealed_source: heights.sealed_source,
      final_height: heights.final_height,
      final_source: heights.final_source,
      anchor: heights.anchor
    }
  end

  @doc """
  The rung alone, for a surface that shows the badge and not the reason.
  """
  @spec rung(non_neg_integer() | nil, Heights.t()) :: rung()
  def rung(block_number, %Heights{} = heights), do: of(block_number, heights).rung

  defp decide(nil, _heights), do: {:pending, :no_block}

  defp decide(block_number, %Heights{} = heights) do
    cond do
      above?(block_number, heights.instant_height) -> {:pending, :above_head}
      covers?(block_number, heights.final_height) -> {:final, final_reason(heights.final_source)}
      covers?(block_number, heights.sealed_height) -> {:sealed, :sealed_by_anchor}
      is_nil(heights.instant_height) -> {:instant, :head_unknown}
      true -> {:instant, :in_consensus_block}
    end
  end

  defp above?(_block_number, nil), do: false
  defp above?(block_number, instant_height) when is_integer(instant_height), do: block_number > instant_height

  defp covers?(_block_number, nil), do: false
  defp covers?(block_number, height) when is_integer(height), do: block_number <= height

  defp final_reason(:confirmations), do: :final_by_confirmations
  defp final_reason(_source), do: :final_by_anchor
end
