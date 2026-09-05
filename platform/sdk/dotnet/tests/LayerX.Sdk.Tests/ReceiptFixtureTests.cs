using System.Runtime.CompilerServices;
using System.Text.Json;
using LayerX.Sdk;
using Xunit;

namespace LayerX.Sdk.Tests;

public sealed class ReceiptFixtureTests
{
    private const string ProgramOutcomeV3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000";

    [Fact]
    public void NativeSignedBinding()
    {
        using var document = JsonDocument.Parse(File.ReadAllText(FixturePath("native-program-call-v3.json")));
        var root = document.RootElement;
        var payload = HexField(root, "payload_hex"); var nativeCall = NativeProgramCall.Decode(payload);
        Assert.Equal(payload, nativeCall.Encode()); var signed = HexField(root, "signed_activity_hex");
        var method = typeof(ProgramsClient).GetMethod("DecodeSignedCall", System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
        method.Invoke(null, new object[] { new ProgramCall(nativeCall, new ProtocolAmount("1000"), signed) });
        Assert.Throws<System.Reflection.TargetInvocationException>(() => method.Invoke(null, new object[] { new ProgramCall(nativeCall, new ProtocolAmount("999"), signed) }));
        Assert.Throws<System.Reflection.TargetInvocationException>(() => method.Invoke(null, new object[] { new ProgramCall(nativeCall with { ResponseCapacity = 17 }, new ProtocolAmount("1000"), signed) }));
        for (var length = 0; length < payload.Length; length++) Assert.Throws<ArgumentException>(() => NativeProgramCall.Decode(payload[..length]));
    }

    [Fact]
    public async Task ExplicitProtocolThree()
    {
        foreach (var name in new[] { "receipt-positive-v3.json", "receipt-programs-positive-v3.json" })
        {
            var fixture = LoadFixture(name);
            await Assert.ThrowsAsync<PlatformSdkException>(async () => await LocalVerifier.VerifyReceiptAsync(fixture.CanonicalReceipt, fixture.Batch));
            var verified = await LocalVerifier.VerifyReceiptAsync(fixture.CanonicalReceipt, fixture.Batch, protocolVersion: 3);
            Assert.Equal((ushort)3, verified.Receipt.ProtocolVersion);
            Assert.Equal(HexField(fixture.Expected, "receipt_digest_hex"), verified.ReceiptDigest);
            var corrupted = (byte[])fixture.CanonicalReceipt.Clone(); corrupted[^1] ^= 1;
            await Assert.ThrowsAsync<PlatformSdkException>(async () => await LocalVerifier.VerifyReceiptAsync(corrupted, fixture.Batch, protocolVersion: 3));
        }
    }

    [Fact]
    public void ProgramOutcomeV3VectorDecodes()
    {
        var outcome = LocalVerifier.DecodeProgramReceiptOutcome(Convert.FromHexString(ProgramOutcomeV3), 1);
        Assert.Equal((byte)3, outcome.EncodingVersion);
        Assert.Equal((ushort)1, outcome.AbiVersion);
        Assert.Equal(new UInt128Value(0, 16), outcome.FeeUnits);
        Assert.Equal(Enumerable.Repeat((byte)0x11, 32).ToArray(), outcome.CallGraphRoot);
        Assert.Equal(Enumerable.Repeat((byte)0x22, 32).ToArray(), outcome.TerminalPayloadRoot);
    }
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

    private static string FixturePath(string name) => Path.Combine(
        RepoRoot(), "platform", "sdk", "conformance", "fixtures", name);

    private static Fixture LoadFixture(string name = "receipt-positive-v2.json")
    {
        var path = FixturePath(name);
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

    [Fact]
    public async Task ProgramsReceiptPreservesOptionalOutcome()
    {
        var fixture = LoadFixture("receipt-programs-positive-v2.json");
        var verified = await LocalVerifier.VerifyReceiptAsync(fixture.CanonicalReceipt, fixture.Batch);
        var outcome = Assert.IsType<ProgramReceiptOutcome>(verified.Receipt.ProgramOutcome);
        Assert.Equal((byte)3, outcome.EncodingVersion);
        Assert.Equal((ushort)1, outcome.RuntimeVersion);
        Assert.Equal((ushort)1, outcome.AbiVersion);
        Assert.Equal(new UInt128Value(0, 2), outcome.OccupancyByteBatches);
        Assert.Equal(new UInt128Value(0, 7), outcome.OccupancyFeeUnits);
        Assert.Equal(fixture.Batch.Asset, outcome.OccupancyAssetId);
        Assert.Contains(outcome.OccupancyEvidenceDigest, value => value != 0);
        Assert.Contains(outcome.OccupancyTransferRoot, value => value != 0);
        Assert.Equal(new UInt128Value(0, 16), outcome.FeeUnits);
    }

    [Fact]
    public async Task RefusalVectorsExposeSharedTaxonomy()
    {
        using var document = JsonDocument.Parse(File.ReadAllText(
            FixturePath("receipt-refusals-v2.json")));
        var root = document.RootElement;
        var authority = root.GetProperty("authorized_batch");
        var batch = new AuthorizedReceiptBatch(
            HexField(authority, "batch_id_hex"),
            HexField(authority, "asset_hex"),
            HexField(authority, "previous_state_root_hex"),
            HexField(authority, "resulting_state_root_hex"),
            HexField(authority, "sequencer_public_key_hex"));
        foreach (var vector in root.GetProperty("vectors").EnumerateArray())
        {
            var failure = await Assert.ThrowsAsync<PlatformSdkException>(async () =>
                await LocalVerifier.VerifyReceiptAsync(
                    HexField(vector, "canonical_receipt_hex"), batch));
            Assert.NotNull(failure.ReceiptCheck);
            Assert.Equal(vector.GetProperty("expected_check").GetString(),
                failure.ReceiptCheck!.Value.MachineCode());
        }
    }
}
