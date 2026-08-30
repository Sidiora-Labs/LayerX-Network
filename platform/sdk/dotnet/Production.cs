#nullable enable

using System.Globalization;
using System.Numerics;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace LayerX.Sdk;

public enum PlatformPlane { Agent, Human }
public enum SdkHttpMethod { Get, Post, Put, Patch, Delete }
public enum RetryClass { Never, Safe, After, UnknownOutcome }

public enum SdkErrorCode
{
    InvalidArgument,
    IdempotencyRequired,
    TransportFailure,
    Deadline,
    ProtocolIncompatibility,
    UnavailableCapability,
    CoreRejection,
    VerificationFailure,
    PolicyRefusal,
    CapabilityRefusal,
    BudgetRefusal,
    RateLimit,
    IdempotencyConflict,
    DecodeFailure,
    UnknownOutcome,
    InternalFault,
}

public static class SdkErrorCodes
{
    public static string MachineCode(this SdkErrorCode code) => code switch
    {
        SdkErrorCode.InvalidArgument => "invalid-argument",
        SdkErrorCode.IdempotencyRequired => "idempotency-required",
        SdkErrorCode.TransportFailure => "transport-failure",
        SdkErrorCode.Deadline => "deadline",
        SdkErrorCode.ProtocolIncompatibility => "protocol-incompatibility",
        SdkErrorCode.UnavailableCapability => "unavailable-capability",
        SdkErrorCode.CoreRejection => "core-rejection",
        SdkErrorCode.VerificationFailure => "verification-failure",
        SdkErrorCode.PolicyRefusal => "policy-refusal",
        SdkErrorCode.CapabilityRefusal => "capability-refusal",
        SdkErrorCode.BudgetRefusal => "budget-refusal",
        SdkErrorCode.RateLimit => "rate-limit",
        SdkErrorCode.IdempotencyConflict => "idempotency-conflict",
        SdkErrorCode.DecodeFailure => "decode-failure",
        SdkErrorCode.UnknownOutcome => "unknown-outcome",
        SdkErrorCode.InternalFault => "internal-fault",
        _ => throw new ArgumentOutOfRangeException(nameof(code)),
    };
}

public sealed class PlatformSdkException : Exception
{
    public SdkErrorCode Code { get; }
    public RetryClass Retry { get; }
    public string? RequestId { get; }
    public int? ProtocolResultCode { get; }
    public ulong? RetryAfterMilliseconds { get; }
    public ReceiptCheck? ReceiptCheck { get; }

    public PlatformSdkException(SdkErrorCode code, RetryClass retry, string? requestId = null, int? protocolResultCode = null, ulong? retryAfterMilliseconds = null, ReceiptCheck? receiptCheck = null)
        : base(SafeMessage(code))
    {
        Code = code;
        Retry = retry;
        RequestId = requestId;
        ProtocolResultCode = protocolResultCode;
        RetryAfterMilliseconds = retryAfterMilliseconds;
        ReceiptCheck = receiptCheck;
    }

    private static string SafeMessage(SdkErrorCode code) => code switch
    {
        SdkErrorCode.InvalidArgument => "The SDK rejected an invalid argument.",
        SdkErrorCode.IdempotencyRequired => "This operation requires an idempotency key.",
        SdkErrorCode.TransportFailure => "The request could not reach the service.",
        SdkErrorCode.Deadline => "The request deadline elapsed.",
        SdkErrorCode.VerificationFailure => "Local verification failed.",
        SdkErrorCode.UnknownOutcome => "The request outcome is unknown and must be resolved before retrying.",
        _ => "The LayerX SDK refused the operation.",
    };
}

public readonly record struct IdempotencyKey
{
    public string Value { get; }

    public IdempotencyKey(string value)
    {
        if (string.IsNullOrEmpty(value) || value.Length > 255 || value.Contains('\0'))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        Value = value;
    }

    internal bool IsValid => !string.IsNullOrEmpty(Value);
    public override string ToString() => Value ?? string.Empty;
}

[JsonConverter(typeof(ProtocolAmountJsonConverter))]
public readonly record struct ProtocolAmount
{
    private static readonly BigInteger Maximum = (BigInteger.One << 128) - BigInteger.One;
    public BigInteger Value { get; }

    public ProtocolAmount(string decimalValue)
    {
        if (string.IsNullOrEmpty(decimalValue) ||
            (decimalValue.Length > 1 && decimalValue[0] == '0') ||
            decimalValue.Any(character => character is < '0' or > '9') ||
            !BigInteger.TryParse(decimalValue, NumberStyles.None, CultureInfo.InvariantCulture, out var value) ||
            value < BigInteger.Zero || value > Maximum)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        Value = value;
    }

    public ProtocolAmount(BigInteger value)
    {
        if (value < BigInteger.Zero || value > Maximum)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        Value = value;
    }

    public override string ToString() => Value.ToString(CultureInfo.InvariantCulture);
}

public sealed class ProtocolAmountJsonConverter : JsonConverter<ProtocolAmount>
{
    public override ProtocolAmount Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options) =>
        reader.TokenType == JsonTokenType.String
            ? new ProtocolAmount(reader.GetString() ?? string.Empty)
            : throw new JsonException("LayerX amounts must be decimal strings.");

    public override void Write(Utf8JsonWriter writer, ProtocolAmount value, JsonSerializerOptions options) =>
        writer.WriteStringValue(value.ToString());
}

public sealed class SecretBytes : IDisposable
{
    private readonly object _gate = new();
    private byte[] _storage;
    private bool _destroyed;

    public SecretBytes(ReadOnlySpan<byte> bytes)
    {
        if (bytes.IsEmpty) throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _storage = bytes.ToArray();
    }

    public T Use<T>(Func<ReadOnlyMemory<byte>, T> consume)
    {
        ArgumentNullException.ThrowIfNull(consume);
        lock (_gate)
        {
            if (_destroyed) throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
            return consume(_storage);
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_destroyed) return;
            CryptographicOperations.ZeroMemory(_storage);
            _storage = Array.Empty<byte>();
            _destroyed = true;
        }
        GC.SuppressFinalize(this);
    }

    ~SecretBytes() => Dispose();
    public override string ToString() => "[REDACTED]";
}

public sealed record OperationDescriptor(
    PlatformPlane Plane,
    string Name,
    SdkHttpMethod Method,
    string Path,
    string RequestType,
    string ResponseType,
    bool RequiresIdempotency,
    bool Bodyless);

public sealed record TransportCall(
    PlatformOperation Operation,
    JsonValue Request,
    IReadOnlyDictionary<string, string> PathParameters,
    IdempotencyKey? IdempotencyKey);

public sealed record ProgramTransportCall(
    string Operation,
    JsonValue Request,
    IReadOnlyDictionary<string, string> PathParameters,
    IdempotencyKey? IdempotencyKey);

public interface IPlatformTransport
{
    Task<JsonValue> SendAsync(TransportCall call, CancellationToken cancellationToken = default);

    Task<JsonValue> SendProgramAsync(ProgramTransportCall call, CancellationToken cancellationToken = default) =>
        Task.FromException<JsonValue>(new PlatformSdkException(SdkErrorCode.UnavailableCapability, RetryClass.Never));
}

public sealed record SdkTelemetryEvent(PlatformPlane Plane, string Operation, string Outcome, SdkErrorCode? Code = null);
public sealed record SdkMetadata(string Name, string Version, int AgentOperations, int HumanOperations);

public sealed class PlatformClient
{
    private readonly IPlatformTransport _transport;
    private readonly Action<SdkTelemetryEvent>? _telemetry;

    public PlatformClient(IPlatformTransport transport, Action<SdkTelemetryEvent>? telemetry = null)
    {
        _transport = transport ?? throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _telemetry = telemetry;
    }

    public Task<JsonValue> ReadAsync(PlatformOperation operation, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default)
    {
        if (operation.Descriptor().RequiresIdempotency)
            throw new PlatformSdkException(SdkErrorCode.IdempotencyRequired, RetryClass.Never);
        return ExecuteAsync(operation, request ?? JsonValue.EmptyObject, null, pathParameters, cancellationToken);
    }

    public Task<JsonValue> MutateAsync(PlatformOperation operation, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default)
    {
        if (!operation.Descriptor().RequiresIdempotency || !idempotencyKey.IsValid)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        return ExecuteAsync(operation, request ?? throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never), idempotencyKey, pathParameters, cancellationToken);
    }

    internal async Task<JsonValue> ProgramAsync(string operation, JsonValue request, IdempotencyKey? idempotencyKey = null,
        IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default)
    {
        try
        {
            var response = await _transport.SendProgramAsync(new ProgramTransportCall(operation,
                request ?? throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never),
                pathParameters is null ? new Dictionary<string, string>() :
                    new Dictionary<string, string>(pathParameters, StringComparer.Ordinal), idempotencyKey),
                cancellationToken).ConfigureAwait(false);
            _telemetry?.Invoke(new(PlatformPlane.Agent, operation, "completed"));
            return response;
        }
        catch (PlatformSdkException exception)
        {
            _telemetry?.Invoke(new(PlatformPlane.Agent, operation, "refused", exception.Code));
            throw;
        }
        catch
        {
            var error = operation == "program.call"
                ? new PlatformSdkException(SdkErrorCode.UnknownOutcome, RetryClass.UnknownOutcome)
                : new PlatformSdkException(SdkErrorCode.TransportFailure, RetryClass.Safe);
            _telemetry?.Invoke(new(PlatformPlane.Agent, operation, "refused", error.Code));
            throw error;
        }
    }

    private async Task<JsonValue> ExecuteAsync(PlatformOperation operation, JsonValue request, IdempotencyKey? idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters, CancellationToken cancellationToken)
    {
        try
        {
            var response = await _transport.SendAsync(new TransportCall(
                operation, request,
                pathParameters is null ? new Dictionary<string, string>() : new Dictionary<string, string>(pathParameters, StringComparer.Ordinal),
                idempotencyKey), cancellationToken).ConfigureAwait(false);
            _telemetry?.Invoke(new(operation.Descriptor().Plane, operation.Descriptor().Name, "completed"));
            return response;
        }
        catch (PlatformSdkException exception)
        {
            _telemetry?.Invoke(new(operation.Descriptor().Plane, operation.Descriptor().Name, "refused", exception.Code));
            throw;
        }
        catch (OperationCanceledException) when (idempotencyKey is null)
        {
            var safe = new PlatformSdkException(SdkErrorCode.Deadline, RetryClass.Safe);
            _telemetry?.Invoke(new(operation.Descriptor().Plane, operation.Descriptor().Name, "refused", safe.Code));
            throw safe;
        }
        catch when (idempotencyKey is not null)
        {
            var safe = new PlatformSdkException(SdkErrorCode.UnknownOutcome, RetryClass.UnknownOutcome);
            _telemetry?.Invoke(new(operation.Descriptor().Plane, operation.Descriptor().Name, "refused", safe.Code));
            throw safe;
        }
        catch
        {
            var safe = new PlatformSdkException(SdkErrorCode.TransportFailure, RetryClass.Safe);
            _telemetry?.Invoke(new(operation.Descriptor().Plane, operation.Descriptor().Name, "refused", safe.Code));
            throw safe;
        }
    }
}
