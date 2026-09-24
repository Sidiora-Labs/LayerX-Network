defmodule Explorer.Chain.PaxeerX.StatusTest.AnchorLive do
  @moduledoc false

  @behaviour Explorer.Chain.PaxeerX.Finality.CapabilitySource

  @impl Explorer.Chain.PaxeerX.Finality.CapabilitySource
  def live?(:anchor), do: true
  def live?(_capability), do: false
end

defmodule Explorer.Chain.PaxeerX.StatusTest.AnchorOff do
  @moduledoc false

  @behaviour Explorer.Chain.PaxeerX.Finality.CapabilitySource

  @impl Explorer.Chain.PaxeerX.Finality.CapabilitySource
  def live?(_capability), do: false
end

defmodule Explorer.Chain.PaxeerX.StatusTest do
  use Explorer.DataCase, async: false

  alias Explorer.Chain.PaxeerX.Finality
  alias Explorer.Chain.PaxeerX.Finality.{Anchor, Heights}
  alias Explorer.Chain.PaxeerX.Status
  alias Explorer.Chain.PaxeerX.StatusTest.{AnchorLive, AnchorOff}

  setup do
    original = Application.get_env(:explorer, Finality)

    on_exit(fn ->
      if original do
        Application.put_env(:explorer, Finality, original)
      else
        Application.delete_env(:explorer, Finality)
      end
    end)

    :ok
  end

  defp heights(fields) do
    struct!(Heights, fields)
  end

  defp anchor(fields) do
    struct!(Anchor, Keyword.put_new(fields, :source, :anchor_table))
  end

  describe "of/2 pending rung" do
    test "an item no block holds is pending" do
      status = Status.of(nil, heights(instant_height: 1_000, sealed_height: 900, final_height: 800))

      assert %Status{rung: :pending, reason: :no_block, block_number: nil} = status
    end

    test "a block number above the head is pending" do
      status = Status.of(1_001, heights(instant_height: 1_000))

      assert %Status{rung: :pending, reason: :above_head, instant_height: 1_000} = status
    end
  end

  describe "of/2 instant rung" do
    test "a block number the head covers is instant" do
      status = Status.of(1_000, heights(instant_height: 1_000))

      assert %Status{rung: :instant, reason: :in_consensus_block} = status
    end

    test "a block number with no head to compare against is instant" do
      status = Status.of(7, heights(instant_height: nil))

      assert %Status{rung: :instant, reason: :head_unknown} = status
    end

    test "an unreported sealed height leaves the sealed rung out of reach rather than standing in a zero" do
      status = Status.of(0, heights(instant_height: 1_000, sealed_height: nil, sealed_source: :no_anchor))

      assert %Status{rung: :instant, reason: :in_consensus_block, sealed_source: :no_anchor} = status
    end

    test "a failed anchor read is carried in the status rather than promoting the rung" do
      status =
        Status.of(10, heights(instant_height: 1_000, sealed_source: {:error, :timeout}))

      assert %Status{rung: :instant, sealed_source: {:error, :timeout}} = status
    end
  end

  describe "of/2 sealed rung" do
    test "a block number the latest anchor seals is sealed" do
      anchor = anchor(checkpoint_height: 900, sealed_height: 900, block_number: 901)

      status =
        Status.of(
          900,
          heights(instant_height: 1_000, sealed_height: 900, sealed_source: :anchor_table, anchor: anchor)
        )

      assert %Status{rung: :sealed, reason: :sealed_by_anchor, sealed_height: 900, anchor: ^anchor} = status
    end

    test "a block number above the sealed height is only instant" do
      status = Status.of(901, heights(instant_height: 1_000, sealed_height: 900, sealed_source: :anchor_call))

      assert %Status{rung: :instant, reason: :in_consensus_block} = status
    end
  end

  describe "of/2 final rung" do
    test "a block number at or below the anchor's finality height is final" do
      status =
        Status.of(
          800,
          heights(
            instant_height: 1_000,
            sealed_height: 900,
            sealed_source: :anchor_call,
            final_height: 800,
            final_source: :anchor
          )
        )

      assert %Status{rung: :final, reason: :final_by_anchor, final_height: 800} = status
    end

    test "the highest rung that holds wins" do
      status =
        Status.of(
          10,
          heights(
            instant_height: 1_000,
            sealed_height: 900,
            sealed_source: :anchor_call,
            final_height: 800,
            final_source: :anchor
          )
        )

      assert %Status{rung: :final} = status
    end

    test "the confirmation fallback names itself as the source" do
      status =
        Status.of(
          900,
          heights(instant_height: 1_000, final_height: 900, final_source: :confirmations)
        )

      assert %Status{rung: :final, reason: :final_by_confirmations, final_source: :confirmations} = status
    end

    test "an unreported finality height leaves the final rung out of reach" do
      status =
        Status.of(
          0,
          heights(instant_height: 1_000, sealed_height: 900, sealed_source: :anchor_table, final_source: :none)
        )

      assert %Status{rung: :sealed} = status
    end
  end

  describe "rung/2" do
    test "answers the rung alone" do
      assert Status.rung(nil, heights([])) == :pending
      assert Status.rung(5, heights(instant_height: 10)) == :instant
    end
  end

  describe "latest_anchor/0" do
    test "answers :no_anchor while the lx_anchors table has not been migrated yet" do
      refute lx_anchors_exists?()

      assert Finality.latest_anchor() == :no_anchor
    end
  end

  describe "heights/1" do
    test "falls back to the lx_anchors source while the anchor capability is not live" do
      Application.put_env(:explorer, Finality, capability_source: AnchorOff)

      assert %Heights{sealed_height: nil, sealed_source: :no_anchor, anchor: nil, final_source: :none} =
               Finality.heights(instant_height: 1_000)
    end

    test "an anchor read supplies both the sealed and the final height" do
      anchor =
        anchor(
          source: :anchor_call,
          batch_number: 19,
          checkpoint_height: 900,
          sealed_height: 900,
          finalized_height: 800
        )

      heights = Finality.heights(instant_height: 1_000, anchor: {:ok, anchor})

      assert %Heights{sealed_height: 900, sealed_source: :anchor_call, final_height: 800, final_source: :anchor} =
               heights

      assert Status.of(850, heights).rung == :sealed
      assert Status.of(800, heights).rung == :final
    end

    test "the confirmation depth is used only when the anchor reports no finalized checkpoint" do
      Application.put_env(:explorer, Finality, capability_source: AnchorOff, final_confirmations: 12)

      assert %Heights{final_height: 988, final_source: :confirmations} = Finality.heights(instant_height: 1_000)

      anchor = anchor(source: :anchor_call, sealed_height: 900, finalized_height: 800)

      assert %Heights{final_height: 800, final_source: :anchor} =
               Finality.heights(instant_height: 1_000, anchor: {:ok, anchor})
    end

    test "the default confirmation depth leaves the final rung to the anchor alone" do
      Application.put_env(:explorer, Finality, capability_source: AnchorOff)

      assert Finality.final_confirmations() == 0
      assert %Heights{final_height: nil, final_source: :none} = Finality.heights(instant_height: 1_000)
    end
  end

  describe "anchor_capability_live?/0" do
    test "is false while the capability module is not loaded" do
      Application.put_env(:explorer, Finality, capability_source: Explorer.Chain.PaxeerX.StatusTest.NotLoaded)

      refute Code.ensure_loaded?(Finality.capability_source())
      refute Finality.anchor_capability_live?()
    end

    test "defaults to the capability module the capabilities surface owns" do
      Application.delete_env(:explorer, Finality)

      assert Finality.capability_source() == Explorer.Chain.PaxeerX.Capabilities
    end

    test "asks the configured capability source" do
      Application.put_env(:explorer, Finality, capability_source: AnchorLive)
      assert Finality.anchor_capability_live?()

      Application.put_env(:explorer, Finality, capability_source: AnchorOff)
      refute Finality.anchor_capability_live?()
    end
  end

  defp lx_anchors_exists? do
    %Postgrex.Result{rows: [[exists?]]} =
      Repo.query!("SELECT EXISTS (SELECT FROM pg_tables WHERE tablename = 'lx_anchors')", [])

    exists?
  end
end
