using System.Buffers.Binary;
using System.Numerics;
using System.Reflection;
using System.Text;
using LayerX.Sdk;
using Xunit;

namespace LayerX.Sdk.Tests;

public sealed class ProgramsContractTests
{
    private sealed class ProgramTransport(JsonValue response) : IPlatformTransport
    {
        public Task<JsonValue> SendAsync(TransportCall call, CancellationToken cancellationToken = default) =>
            Task.FromException<JsonValue>(new PlatformSdkException(SdkErrorCode.UnavailableCapability, RetryClass.Never));
        public Task<JsonValue> SendProgramAsync(ProgramTransportCall call, CancellationToken cancellationToken = default) =>
            Task.FromResult(response);
    }

    [Fact]
    public void ProgramsClientRequiresIndependentNonzeroSequencerPin()
    {
        var client = new PlatformClient(new ProgramTransport(JsonValue.EmptyObject));
        Assert.Throws<PlatformSdkException>(() => new ProgramsClient(client, new byte[32]));
        _ = new ProgramsClient(client, Enumerable.Repeat((byte)1, 32).ToArray());
    }

    [Fact]
    public async Task PendingReceiptMayOmitRetainedBytesButMustBindExpectedActivity()
    {
        var key = new string('a', 64); var activity = Enumerable.Repeat((byte)0x11, 32).ToArray();
        var value = JsonValue.Object(new Dictionary<string, JsonValue>
        {
            ["state"] = JsonValue.String("unknown"),
            ["activity_id"] = JsonValue.String(Convert.ToHexString(activity).ToLowerInvariant()),
            ["idempotency_key"] = JsonValue.String(key),
        });
        var programs = new ProgramsClient(new PlatformClient(new ProgramTransport(value)),
            Enumerable.Repeat((byte)1, 32).ToArray());
        var pending = await programs.ReceiptAsync(new IdempotencyKey(key), activity, "sequencer-signed");
        Assert.True(pending.IsUnknown); Assert.Null(pending.RetainedSignedActivity);
        var error = await Assert.ThrowsAsync<PlatformSdkException>(() => programs.ReceiptAsync(new IdempotencyKey(key),
            Enumerable.Repeat((byte)0x12, 32).ToArray(), "sequencer-signed"));
        Assert.Equal(SdkErrorCode.VerificationFailure, error.Code);
    }

    [Fact]
    public void OperationValueVerificationStatusMatrixIsExact()
    {
        var achieved = Status("Achieved", null); var discovery = Status("Unverified", "server_side_receipt_verification_only");
        var pending = Status("Unverified", "receipt_pending");
        var unknown = JsonValue.Object(new Dictionary<string, JsonValue> { ["state"] = JsonValue.String("unknown") });
        Assert.True(TransportStatus("program.discover", JsonValue.EmptyObject, discovery));
        Assert.False(TransportStatus("program.discover", JsonValue.EmptyObject, achieved));
        Assert.True(TransportStatus("program.receipt", unknown, pending));
        Assert.False(TransportStatus("program.receipt", unknown, achieved));
        Assert.True(TransportStatus("program.simulate", JsonValue.EmptyObject, achieved));
        Assert.False(TransportStatus("program.simulate", JsonValue.EmptyObject, discovery));
    }

    [Fact]
    public void TransferSetV1AndV2ProduceTheSameCanonicalKernelRoot()
    {
        var v1 = TransferAuthorization(1); var v2 = TransferAuthorization(2);
        var rootV1 = (byte[])Invoke("DecodeAuthorizationRoot", v1)!;
        var rootV2 = (byte[])Invoke("DecodeAuthorizationRoot", v2)!;
        Assert.Equal(rootV1, rootV2); Assert.Contains(rootV1, value => value != 0);
        var mutated = v2.ToArray(); mutated[^33] ^= 1;
        Assert.NotEqual(rootV2, (byte[])Invoke("DecodeAuthorizationRoot", mutated)!);
    }

    [Fact]
    public void OccupancyV1V2V3AndAggregateBindingsAreExact()
    {
        var asset = Enumerable.Repeat((byte)0x66, 32).ToArray();
        for (var version = 1; version <= 3; version++)
        {
            var binding = Invoke("DecodeOccupancySettlement", EmptyOccupancy(version))!;
            Assert.Equal(BigInteger.Zero, Property<BigInteger>(binding, "ByteBatches"));
            Assert.Equal(BigInteger.Zero, Property<BigInteger>(binding, "FeeUnits"));
            Assert.Equal(new byte[32], (byte[])Invoke("OccupancyTransferRoot", binding, asset)!);
        }
        var evidence = ChargedOccupancy(); var charged = Invoke("DecodeOccupancySettlement", evidence)!;
        Assert.Equal(new BigInteger(3), Property<BigInteger>(charged, "ByteBatches"));
        Assert.Equal(new BigInteger(6), Property<BigInteger>(charged, "FeeUnits"));
        Assert.Contains((byte[])Invoke("OccupancyTransferRoot", charged, asset)!, value => value != 0);
        var mutated = evidence.ToArray();
        var declaredFeeLowByte = Encoding.UTF8.GetByteCount("LXP/storage-occupancy-settlement/v3\0") + 8 + 4 + 7 * 8 + 16 + 15;
        mutated[declaredFeeLowByte] ^= 1;
        var exception = Assert.Throws<TargetInvocationException>(() => Invoke("DecodeOccupancySettlement", mutated));
        Assert.IsType<InvalidDataException>(exception.InnerException);
    }

    private static object? Invoke(string name, params object[] arguments)
    {
        var method = typeof(ProgramsClient).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException($"missing {name}");
        return method.Invoke(null, arguments);
    }

    private static bool TransportStatus(string operation, JsonValue value, JsonValue status)
    {
        var method = typeof(AgentHttpTransport).GetMethod("ValidProgramVerification", BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("missing ValidProgramVerification");
        return (bool)(method.Invoke(null, [operation, value, status]) ?? false);
    }

    private static JsonValue Status(string state, string? reason)
    {
        var fields = new Dictionary<string, JsonValue>
        {
            ["state"] = JsonValue.String(state), ["level"] = JsonValue.String("SequencerSigned"),
        };
        if (reason is not null) fields["reason"] = JsonValue.String(reason); return JsonValue.Object(fields);
    }

    private static T Property<T>(object value, string name) => (T)(value.GetType().GetProperty(name)?.GetValue(value)
        ?? throw new InvalidOperationException($"missing {name}"));

    private static byte[] TransferAuthorization(int version)
    {
        var program = Repeat(1); var principal = Repeat(2); var asset = Repeat(4); var destination = Repeat(5);
        using var stream = new MemoryStream(); Write(stream, Encoding.UTF8.GetBytes($"LayerX/programs/402LXP/transfer-set/v{version}\0"));
        Write(stream, program); Write(stream, principal); Write(stream, Repeat(3)); Write(stream, new byte[9]);
        using var events = new MemoryStream(); Write(events, Encoding.UTF8.GetBytes("LayerX/programs/events/v1\0")); Write(events, Be(0, 4));
        Write(stream, Be((ulong)events.Length, 4)); Write(stream, events.ToArray()); Write(stream, Be(0, 8)); Write(stream, Be(1, 8));
        Write(stream, new byte[9]); if (version == 2) { stream.WriteByte(1); Write(stream, principal); }
        Write(stream, asset); Write(stream, destination); Write(stream, U128(7)); Write(stream, program); return stream.ToArray();
    }

    private static byte[] EmptyOccupancy(int version)
    {
        using var stream = new MemoryStream(); Write(stream, Encoding.UTF8.GetBytes($"LXP/storage-occupancy-settlement/v{version}\0"));
        Write(stream, Be(1, 8)); if (version > 1) Write(stream, Be(1, 4));
        for (ulong value = 1; value <= 7; value++) Write(stream, Be(value, 8));
        if (version == 3) { Write(stream, new byte[16 * 4]); Write(stream, Be(0, 4)); }
        else { Write(stream, new byte[16 * 2]); Write(stream, Be(0, 8)); }
        return stream.ToArray();
    }

    private static byte[] ChargedOccupancy()
    {
        var program = Repeat(0x11); var payer = Repeat(0x77); using var stream = new MemoryStream();
        Write(stream, Encoding.UTF8.GetBytes("LXP/storage-occupancy-settlement/v3\0")); Write(stream, Be(2, 8)); Write(stream, Be(1, 4));
        foreach (ulong value in new ulong[] { 0, 0, 0, 0, 0, 0, 2 }) Write(stream, Be(value, 8));
        Write(stream, U128(3)); Write(stream, U128(6)); Write(stream, U128(6)); Write(stream, U128(0)); Write(stream, Be(1, 4));
        stream.WriteByte(65); Write(stream, program); stream.WriteByte(0); Write(stream, payer);
        Write(stream, payer); Write(stream, program); Write(stream, Repeat(0x88));
        Write(stream, Be(1, 8)); Write(stream, Be(2, 8)); Write(stream, Be(3, 8)); Write(stream, Be(3, 8));
        Write(stream, U128(3)); Write(stream, Be(2, 8)); Write(stream, U128(6)); Write(stream, U128(0));
        Write(stream, U128(6)); Write(stream, U128(0)); stream.WriteByte(1); Write(stream, U128(0));
        Write(stream, Be(3, 8)); Write(stream, Be(2, 8)); Write(stream, U128(0)); Write(stream, Repeat(0x99));
        return stream.ToArray();
    }

    private static byte[] Repeat(byte value) => Enumerable.Repeat(value, 32).ToArray();
    private static byte[] U128(ulong value) => new byte[8].Concat(Be(value, 8)).ToArray();
    private static byte[] Be(ulong value, int length)
    {
        var encoded = new byte[8]; BinaryPrimitives.WriteUInt64BigEndian(encoded, value); return encoded[(8 - length)..];
    }
    private static void Write(Stream stream, byte[] value) => stream.Write(value);
}
