using System.Runtime.CompilerServices;
using System.Text.Json;
using LayerX.Sdk;
using Xunit;

namespace LayerX.Sdk.Tests;

public sealed class ReceiptFixtureTests
{
    private sealed record Fixture(
        byte[] CanonicalReceipt,
        AuthorizedReceiptBatch Batch,
        JsonElement Expected,
        JsonElement AuthorizedBatch);

    private static string RepoRoot([CallerFilePath] string sourcePath = "")
        => Path.GetFullPath(Path.Combine(
            Path.GetDirectoryName(sourcePath) ?? throw new InvalidOperationException("no source dir"),
            "..", "..", "..", "..", ".."));

    private static byte[] HexField(JsonElement element, string key)
        => Convert.FromHexString(element.GetProperty(key).GetString()
            ?? throw new InvalidOperationException($"missing {key}"));

    private static UInt128Value U128Field(JsonElement element, string key)
        => new(0, ulong.Parse(element.GetProperty(key).GetString()
            ?? throw new InvalidOperationException($"missing {key}")));

    private static Fixture LoadFixture()
    {
        var path = Path.Combine(
            RepoRoot(), "platform", "sdk", "conformance", "fixtures", "receipt-positive-v1.json");
        using var document = JsonDocument.Parse(File.ReadAllText(path));
        var root = document.RootElement.Clone();
        var batch = root.GetProperty("authorized_batch");
        return new Fixture(
            HexField(root, "canonical_receipt_hex"),
            new AuthorizedReceiptBatch(
                HexField(batch, "batch_id_hex"),
                HexField(batch, "asset_hex"),
                HexField(batch, "previous_state_root_hex"),
                HexField(batch, "resulting_state_root_hex"),
                HexField(batch, "sequencer_public_key_hex")),
            root.GetProperty("expected"),
            batch);
    }

    [Fact]
    public async Task CoreFixtureReceiptVerifiesPositively()
    {
        var fixture = LoadFixture();
        var expected = fixture.Expected;
        var verified = await LocalVerifier.VerifyReceiptAsync(fixture.CanonicalReceipt, fixture.Batch);
        Assert.Equal(expected.GetProperty("level").GetString(), verified.Level);
        Assert.Equal(fixture.CanonicalReceipt, verified.CanonicalBytes);
        Assert.Equal(HexField(expected, "receipt_digest_hex"), verified.ReceiptDigest);
        var receipt = verified.Receipt;
        Assert.Equal(expected.GetProperty("result_code").GetInt32(), receipt.ResultCode);
        Assert.Equal(expected.GetProperty("protocol_version").GetUInt16(), receipt.ProtocolVersion);
        Assert.Equal(expected.GetProperty("operation").GetByte(), receipt.Operation);
        Assert.Equal(expected.GetProperty("module_id").GetUInt16(), receipt.ModuleId);
        Assert.Equal(expected.GetProperty("global_sequence").GetUInt64(), receipt.GlobalSequence);
        Assert.Equal(expected.GetProperty("timestamp_ms").GetUInt64(), receipt.Timestamp);
        Assert.Equal(U128Field(expected, "amount"), receipt.Amount);
        Assert.Equal(U128Field(expected, "fee_charged"), receipt.FeeCharged);
        Assert.Equal(U128Field(expected, "from_balance_before"), receipt.FromBalanceBefore);
        Assert.Equal(U128Field(expected, "from_balance_after"), receipt.FromBalanceAfter);
        Assert.Equal(U128Field(expected, "to_balance_before"), receipt.ToBalanceBefore);
        Assert.Equal(U128Field(expected, "to_balance_after"), receipt.ToBalanceAfter);
        Assert.Equal(HexField(expected, "activity_id_hex"), receipt.ActivityId);
        Assert.Equal(HexField(expected, "from_hex"), receipt.From);
        Assert.Equal(HexField(expected, "to_hex"), receipt.To);
        Assert.Equal(HexField(fixture.AuthorizedBatch, "batch_id_hex"), receipt.BatchId);
        Assert.Equal(HexField(fixture.AuthorizedBatch, "asset_hex"), receipt.Asset);
        Assert.Equal(
            HexField(fixture.AuthorizedBatch, "previous_state_root_hex"), receipt.PreviousStateRoot);
        Assert.Equal(
            HexField(fixture.AuthorizedBatch, "resulting_state_root_hex"), receipt.ResultingStateRoot);
    }

    [Fact]
    public async Task CoreFixtureReceiptByteFlipFails()
    {
        var fixture = LoadFixture();
        var mutated = (byte[])fixture.CanonicalReceipt.Clone();
        mutated[^1] ^= 0x01;
        await Assert.ThrowsAsync<PlatformSdkException>(async () =>
            await LocalVerifier.VerifyReceiptAsync(mutated, fixture.Batch));
    }
}
