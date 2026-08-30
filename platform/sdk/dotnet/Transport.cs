#nullable enable

using System.Net;
using System.Net.Http.Headers;
using System.Numerics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace LayerX.Sdk;

public sealed class AccessToken : IDisposable
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private readonly SecretBytes _secret;

    public AccessToken(ReadOnlySpan<byte> bytes)
    {
        try { _ = StrictUtf8.GetCharCount(bytes); }
        catch (DecoderFallbackException) { throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never); }
        _secret = new SecretBytes(bytes);
    }

    internal void Authorize(HttpRequestMessage request) => _secret.Use(bytes =>
    {
        var value = StrictUtf8.GetString(bytes.Span);
        if (value.Contains('\r') || value.Contains('\n'))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", value);
        return true;
    });

    public void Dispose() => _secret.Dispose();
    public override string ToString() => "[REDACTED]";
}

public sealed class LayerXKeyCredential : IDisposable
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private readonly string _keyId;
    private readonly SecretBytes _secret;

    public LayerXKeyCredential(string keyId, ReadOnlySpan<byte> secret)
    {
        if (string.IsNullOrEmpty(keyId) || keyId.Length > 64 || keyId.Any(character =>
            !(character is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or >= '0' and <= '9' or '-' or '_')))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _keyId = keyId;
        _secret = new SecretBytes(secret);
    }

    internal void Authorize(HttpRequestMessage request) => _secret.Use(bytes =>
    {
        string value;
        try { value = StrictUtf8.GetString(bytes.Span); }
        catch (DecoderFallbackException) { throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never); }
        if (!value.StartsWith("lxp_live_", StringComparison.Ordinal) || value.Length != 73 ||
            value.Skip(9).Any(character => !(character is >= '0' and <= '9' or >= 'a' and <= 'f')))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        request.Headers.TryAddWithoutValidation("Authorization", $"LayerX-Key {_keyId}:{value}");
        return true;
    });

    public void Dispose() => _secret.Dispose();
    public override string ToString() => "[REDACTED]";
}

public sealed class AgentHttpTransport : IPlatformTransport
{
    private const int MaximumResponseBytes = 8 * 1024 * 1024;
    private const int MaximumProgramsRequestBytes = 8 * 1024 * 1024;
    private const int MaximumProgramBytes = 1_048_576;
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private static readonly HashSet<string> Operations = new(StringComparer.Ordinal)
    {
        "program.discover", "program.interface", "program.simulate",
        "program.call", "program.receipt", "program.activity",
    };
    private readonly Uri _baseUri;
    private readonly HttpClient _httpClient;
    private readonly LayerXKeyCredential? _credential;
    private sealed record ProgramRoute(HttpMethod Method, string Path,
        IReadOnlySet<string> PathParameters, bool Idempotent);
    private static readonly IReadOnlyDictionary<string, ProgramRoute> ProgramRoutes =
        new Dictionary<string, ProgramRoute>(StringComparer.Ordinal)
        {
            ["program.discover"] = new(HttpMethod.Get, "/v1/programs/registry/{program_id}",
                new HashSet<string>(["program_id"], StringComparer.Ordinal), false),
            ["program.interface"] = new(HttpMethod.Get, "/v1/programs/registry/{program_id}/interface",
                new HashSet<string>(["program_id"], StringComparer.Ordinal), false),
            ["program.simulate"] = new(HttpMethod.Post, "/v1/programs/simulate",
                new HashSet<string>(StringComparer.Ordinal), false),
            ["program.call"] = new(HttpMethod.Post, "/v1/programs/call",
                new HashSet<string>(StringComparer.Ordinal), true),
            ["program.receipt"] = new(HttpMethod.Get, "/v1/programs/receipts/by-idempotency/{idempotency_key}",
                new HashSet<string>(["idempotency_key"], StringComparer.Ordinal), false),
            ["program.activity"] = new(HttpMethod.Get, "/v1/programs/activities/{activity_id}",
                new HashSet<string>(["activity_id"], StringComparer.Ordinal), false),
        };

    public AgentHttpTransport(Uri baseUri, HttpClient? httpClient = null, LayerXKeyCredential? credential = null)
    {
        if (!baseUri.IsAbsoluteUri || !string.IsNullOrEmpty(baseUri.UserInfo) || string.IsNullOrEmpty(baseUri.Host) ||
            !string.IsNullOrEmpty(baseUri.Query) || !string.IsNullOrEmpty(baseUri.Fragment) ||
            (baseUri.Scheme != Uri.UriSchemeHttps && (baseUri.Scheme != Uri.UriSchemeHttp || !IsLoopback(baseUri.Host))))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        if (httpClient is not null)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _baseUri = baseUri;
        _httpClient = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false });
        _credential = credential;
    }

    public async Task<JsonValue> SendAsync(TransportCall call, CancellationToken cancellationToken = default)
    {
        var descriptor = call.Operation.Descriptor();
        if (descriptor.Plane != PlatformPlane.Agent || !Operations.Contains(descriptor.Name))
            throw new PlatformSdkException(SdkErrorCode.UnavailableCapability, RetryClass.Never);
        return await SendProgramAsync(new ProgramTransportCall(descriptor.Name, call.Request,
            call.PathParameters, call.IdempotencyKey), cancellationToken).ConfigureAwait(false);
    }

    public async Task<JsonValue> SendProgramAsync(ProgramTransportCall call, CancellationToken cancellationToken = default)
    {
        if (!ProgramRoutes.TryGetValue(call.Operation, out var route) ||
            !route.PathParameters.SetEquals(call.PathParameters.Keys)) throw Invalid();
        if (route.Idempotent)
        {
            if (call.IdempotencyKey is not { } key || !Hex32(key.Value))
                throw new PlatformSdkException(SdkErrorCode.IdempotencyRequired, RetryClass.Never);
        }
        else if (call.IdempotencyKey is not null)
            throw Invalid();
        ValidateProgramRequest(call);
        var encodedRequest = JsonSerializer.SerializeToUtf8Bytes(call.Request, JsonOptions);
        if (encodedRequest.Length == 0 || encodedRequest.Length > MaximumProgramsRequestBytes) throw Invalid();
        var path = route.Path;
        foreach (var name in route.PathParameters)
        {
            if (!call.PathParameters.TryGetValue(name, out var value) || !Hex32(value) ||
                !ProgramMap(call.Request).TryGetValue(name, out var rawBodyValue) ||
                rawBodyValue is not JsonValue.StringValue bodyValue || bodyValue.Value != value)
                throw Invalid();
            path = path.Replace("{" + name + "}", Uri.EscapeDataString(value), StringComparison.Ordinal);
        }
        var target = RootEndpoint(_baseUri, path);
        using var request = new HttpRequestMessage(route.Method, target);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        request.Headers.UserAgent.ParseAdd("layerx-dotnet/0.1.0");
        request.Content = new ByteArrayContent(encodedRequest);
        request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        if (call.IdempotencyKey is { } idempotency)
            request.Headers.TryAddWithoutValidation("Idempotency-Key", idempotency.Value);
        if (_credential is null) throw new PlatformSdkException(SdkErrorCode.CapabilityRefusal, RetryClass.Never);
        _credential.Authorize(request);
        HttpResponseMessage response;
        try
        {
            response = await _httpClient.SendAsync(request, HttpCompletionOption.ResponseHeadersRead,
                cancellationToken).ConfigureAwait(false);
        }
        catch when (call.Operation == "program.call") { throw UnknownOutcome(); }
        catch (OperationCanceledException) { throw new PlatformSdkException(SdkErrorCode.Deadline, RetryClass.Safe); }
        catch { throw new PlatformSdkException(SdkErrorCode.TransportFailure, RetryClass.Safe); }
        using (response)
        {
            try
            {
                if (response.Content.Headers.ContentType?.MediaType is not string mediaType ||
                    !string.Equals(mediaType, "application/json", StringComparison.OrdinalIgnoreCase)) throw Decode();
                var encoded = await ReadBoundedAsync(response.Content, cancellationToken).ConfigureAwait(false);
                return DecodeProgramEnvelope(call.Operation, (int)response.StatusCode, encoded);
            }
            catch (PlatformSdkException error) when (call.Operation == "program.call" &&
                error.Code is SdkErrorCode.DecodeFailure or SdkErrorCode.VerificationFailure)
            {
                throw UnknownOutcome();
            }
        }
    }

    private static JsonValue DecodeProgramEnvelope(string operation, int status, byte[] encoded)
    {
        JsonValue? document;
        try { document = JsonSerializer.Deserialize<JsonValue>(encoded, JsonOptions); }
        catch (JsonException) { throw Decode(); }
        var envelope = ResponseMap(document ?? throw Decode());
        if (envelope.ContainsKey("class"))
        {
            if (status is >= 200 and < 300 || !Exact(envelope,
                "class", "protocol_result_code", "retriability", "request_id", "reason")) throw Decode();
            throw ProgramServiceError(envelope);
        }
        var requestId = TryText(envelope, "request_id");
        if (status is < 200 or >= 300 || !Exact(envelope, "request_id", "value", "verification_status") ||
            requestId is null || !ValidRequestId(requestId) || !envelope.TryGetValue("value", out var value) || value is JsonValue.NullValue ||
            !envelope.TryGetValue("verification_status", out var verification) ||
            !ValidProgramVerification(operation, value, verification)) throw Decode(requestId);
        return value;
    }

    private static bool ValidProgramVerification(string operation, JsonValue value, JsonValue verification)
    {
        var status = verification is JsonValue.ObjectValue objectValue ? objectValue.Value : null;
        if (status is null) return false;
        if (operation is "program.discover" or "program.interface")
            return Exact(status, "state", "requested", "achieved", "reason") && Text(status, "state") == "Unverified" &&
                Text(status, "requested") == "SequencerSigned" && Text(status, "achieved") == "Unverified" &&
                Text(status, "reason") == "server_side_receipt_verification_only";
        var pending = (operation is "program.call" or "program.receipt" or "program.activity") &&
            value is JsonValue.ObjectValue pendingValue && TryText(pendingValue.Value, "state") is "unknown" or "pending";
        if (pending)
            return Exact(status, "state", "requested", "achieved", "reason") && Text(status, "state") == "Unverified" &&
                Text(status, "requested") == "SequencerSigned" && Text(status, "achieved") == "Unverified" &&
                Text(status, "reason") == "receipt_pending";
        return (operation is "program.simulate" or "program.call" or "program.receipt" or "program.activity") &&
            Exact(status, "state", "level") && Text(status, "state") == "Achieved" &&
            Text(status, "level") == "SequencerSigned";
    }

    private static PlatformSdkException ProgramServiceError(IReadOnlyDictionary<string, JsonValue> envelope)
    {
        var requestId = Text(envelope, "request_id"); var reason = Text(envelope, "reason");
        if (!ValidRequestId(requestId) || string.IsNullOrEmpty(reason) || reason.Length > 128 ||
            reason.Any(character => !(character is >= 'a' and <= 'z' or >= '0' and <= '9' or '_' or '.'))) throw Decode();
        int? resultCode = envelope["protocol_result_code"] switch
        {
            JsonValue.NullValue => null,
            JsonValue.IntegerValue integer when integer.Value is >= int.MinValue and <= int.MaxValue => (int)integer.Value,
            _ => throw Decode(requestId),
        };
        var code = Text(envelope, "class") switch
        {
            "TransportFailure" => SdkErrorCode.TransportFailure, "Deadline" => SdkErrorCode.Deadline,
            "ProtocolIncompatibility" => SdkErrorCode.ProtocolIncompatibility,
            "UnavailableCapability" => SdkErrorCode.UnavailableCapability,
            "CoreRejection" => SdkErrorCode.CoreRejection, "VerificationFailure" => SdkErrorCode.VerificationFailure,
            "PolicyRefusal" => SdkErrorCode.PolicyRefusal, "CapabilityRefusal" => SdkErrorCode.CapabilityRefusal,
            "BudgetRefusal" => SdkErrorCode.BudgetRefusal, "RateLimit" => SdkErrorCode.RateLimit,
            "IdempotencyConflict" => SdkErrorCode.IdempotencyConflict, "InternalFault" => SdkErrorCode.InternalFault,
            _ => throw Decode(requestId),
        };
        var retry = Text(envelope, "retriability") switch
        {
            "Terminal" => RetryClass.Never, "Retriable" => RetryClass.Safe, _ => throw Decode(requestId),
        };
        return new PlatformSdkException(code, retry, requestId, resultCode);
    }

    private static void ValidateProgramRequest(ProgramTransportCall call)
    {
        var value = ProgramMap(call.Request);
        switch (call.Operation)
        {
            case "program.discover": case "program.interface":
                if (!Exact(value, "program_id", "requested_verification_level") ||
                    !CanonicalProgram(value.GetValueOrDefault("program_id")) ||
                    TryText(value, "requested_verification_level") != "sequencer-signed") throw Invalid();
                break;
            case "program.receipt":
                if (!Exact(value, "idempotency_key", "expected_activity_id", "requested_verification_level") ||
                    !CanonicalHex(value.GetValueOrDefault("idempotency_key"), 32, false) ||
                    !CanonicalHex(value.GetValueOrDefault("expected_activity_id"), 32, false) ||
                    TryText(value, "requested_verification_level") != "sequencer-signed") throw Invalid();
                break;
            case "program.activity":
                if (!Exact(value, "activity_id", "requested_verification_level") ||
                    !CanonicalHex(value.GetValueOrDefault("activity_id"), 32, false) ||
                    TryText(value, "requested_verification_level") != "sequencer-signed") throw Invalid();
                break;
            case "program.simulate": case "program.call": ValidateProgramCall(value); break;
            default: throw Invalid();
        }
    }

    private static void ValidateProgramCall(IReadOnlyDictionary<string, JsonValue> value)
    {
        if (!Exact(value, "program_id", "calldata", "budget", "capabilities", "signed_activity") ||
            !CanonicalProgram(value.GetValueOrDefault("program_id")) ||
            !BoundedHex(value.GetValueOrDefault("calldata"), MaximumProgramBytes, true) ||
            !BoundedHex(value.GetValueOrDefault("signed_activity"), MaximumProgramBytes, false) ||
            value.GetValueOrDefault("budget") is not JsonValue.ObjectValue budget ||
            !Exact(budget.Value, "fuel", "fee_limit") || !CanonicalUInt64(budget.Value.GetValueOrDefault("fuel"), true) ||
            !CanonicalUInt128(budget.Value.GetValueOrDefault("fee_limit")) ||
            value.GetValueOrDefault("capabilities") is not JsonValue.ArrayValue capabilities || capabilities.Value.Count > 5)
            throw Invalid();
        string[] order = ["storage_read", "storage_write", "transfer", "emit_event", "compose"];
        var previous = -1;
        foreach (var capability in capabilities.Value)
        {
            var current = capability is JsonValue.StringValue name ? Array.IndexOf(order, name.Value) : -1;
            if (current <= previous) throw Invalid();
            previous = current;
        }
    }

    private static IReadOnlyDictionary<string, JsonValue> ProgramMap(JsonValue value) =>
        value is JsonValue.ObjectValue map ? map.Value : throw Invalid();
    private static IReadOnlyDictionary<string, JsonValue> ResponseMap(JsonValue value) =>
        value is JsonValue.ObjectValue map ? map.Value : throw Decode();
    private static bool Exact(IReadOnlyDictionary<string, JsonValue> value, params string[] fields) =>
        value.Count == fields.Length && fields.All(value.ContainsKey);
    private static string Text(IReadOnlyDictionary<string, JsonValue> value, string field) =>
        TryText(value, field) ?? throw Decode();
    private static string? TryText(IReadOnlyDictionary<string, JsonValue> value, string field) =>
        value.TryGetValue(field, out var raw) && raw is JsonValue.StringValue text ? text.Value : null;
    private static bool CanonicalProgram(JsonValue? value) => CanonicalHex(value, 32, false) &&
        value is JsonValue.StringValue text && text.Value != new string('0', 64);
    private static bool CanonicalHex(JsonValue? value, int bytes, bool empty) =>
        value is JsonValue.StringValue text && (empty && text.Value.Length == 0 ||
            text.Value.Length == bytes * 2 && text.Value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f'));
    private static bool BoundedHex(JsonValue? value, int maximum, bool empty) =>
        value is JsonValue.StringValue text && text.Value.Length % 2 == 0 && text.Value.Length <= maximum * 2 &&
        (empty || text.Value.Length != 0) && text.Value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    private static bool CanonicalUInt64(JsonValue? value, bool positive) => value is JsonValue.StringValue text &&
        CanonicalDecimal(text.Value) && ulong.TryParse(text.Value, out var parsed) && (!positive || parsed > 0);
    private static bool CanonicalUInt128(JsonValue? value) => value is JsonValue.StringValue text &&
        CanonicalDecimal(text.Value) && BigInteger.TryParse(text.Value, out var parsed) &&
        parsed >= BigInteger.Zero && parsed < (BigInteger.One << 128);
    private static bool CanonicalDecimal(string value) => !string.IsNullOrEmpty(value) &&
        (value == "0" || value[0] != '0') && value.All(character => character is >= '0' and <= '9');
    private static bool ValidRequestId(string value) => !string.IsNullOrEmpty(value) && value.Length <= 128 &&
        value.All(character => character is >= (char)0x21 and <= (char)0x7e);

    private static PlatformSdkException ServiceError(AgentEnvelope envelope)
    {
        if (string.IsNullOrEmpty(envelope.RequestId) || string.IsNullOrEmpty(envelope.Reason) ||
            envelope.Reason.Any(character => !(character is >= 'a' and <= 'z' or >= '0' and <= '9' or '_' or '.')))
            throw Decode(envelope.RequestId);
        var code = envelope.ErrorClass switch
        {
            "TransportFailure" => SdkErrorCode.TransportFailure,
            "Deadline" => SdkErrorCode.Deadline,
            "ProtocolIncompatibility" => SdkErrorCode.ProtocolIncompatibility,
            "UnavailableCapability" => SdkErrorCode.UnavailableCapability,
            "CoreRejection" => SdkErrorCode.CoreRejection,
            "VerificationFailure" => SdkErrorCode.VerificationFailure,
            "PolicyRefusal" => SdkErrorCode.PolicyRefusal,
            "CapabilityRefusal" => SdkErrorCode.CapabilityRefusal,
            "BudgetRefusal" => SdkErrorCode.BudgetRefusal,
            "RateLimit" => SdkErrorCode.RateLimit,
            "IdempotencyConflict" => SdkErrorCode.IdempotencyConflict,
            "InternalFault" => SdkErrorCode.InternalFault,
            _ => throw Decode(envelope.RequestId),
        };
        var retry = envelope.Retriability switch
        {
            "Terminal" => RetryClass.Never,
            "Retriable" => RetryClass.Safe,
            _ => throw Decode(envelope.RequestId),
        };
        return new PlatformSdkException(code, retry, envelope.RequestId, envelope.ProtocolResultCode);
    }

    private static async Task<byte[]> ReadBoundedAsync(HttpContent content, CancellationToken cancellationToken)
    {
        await using var stream = await content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
        using var output = new MemoryStream();
        var buffer = new byte[16 * 1024];
        while (true)
        {
            var count = await stream.ReadAsync(buffer.AsMemory(), cancellationToken).ConfigureAwait(false);
            if (count == 0) return output.ToArray();
            if (output.Length + count > MaximumResponseBytes) throw Decode();
            output.Write(buffer, 0, count);
        }
    }

    private static string ResolvePath(string template, IReadOnlyDictionary<string, string> parameters)
    {
        var path = template;
        foreach (var (name, value) in parameters)
        {
            var token = "{" + name + "}";
            if (string.IsNullOrEmpty(name) || string.IsNullOrEmpty(value) || !path.Contains(token, StringComparison.Ordinal))
                throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
            path = path.Replace(token, Uri.EscapeDataString(value), StringComparison.Ordinal);
        }
        if (!path.StartsWith("/", StringComparison.Ordinal) || path.Contains('{') || path.Contains('}'))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        return path;
    }

    private static Uri Endpoint(Uri baseUri, string path)
    {
        var builder = new UriBuilder(baseUri);
        builder.Path = baseUri.AbsolutePath.TrimEnd('/') + path;
        builder.Query = ""; builder.Fragment = "";
        return builder.Uri;
    }

    private static Uri RootEndpoint(Uri baseUri, string path)
    {
        var builder = new UriBuilder(baseUri) { Path = path, Query = "", Fragment = "" };
        return builder.Uri;
    }

    private static HttpMethod ToHttpMethod(SdkHttpMethod method) => method switch
    {
        SdkHttpMethod.Get => HttpMethod.Get, SdkHttpMethod.Post => HttpMethod.Post,
        SdkHttpMethod.Put => HttpMethod.Put, SdkHttpMethod.Patch => HttpMethod.Patch,
        SdkHttpMethod.Delete => HttpMethod.Delete, _ => throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never),
    };

    private static bool IsLoopback(string host) => string.Equals(host, "localhost", StringComparison.OrdinalIgnoreCase) ||
        IPAddress.TryParse(host, out var address) && IPAddress.IsLoopback(address);
    private static bool Hex32(string value) => value.Length == 64 && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
    private static PlatformSdkException Decode(string? requestId = null) => new(SdkErrorCode.DecodeFailure, RetryClass.Never, requestId);
    private static PlatformSdkException UnknownOutcome() => new(SdkErrorCode.UnknownOutcome, RetryClass.UnknownOutcome);

    private sealed record AgentEnvelope(
        [property: JsonPropertyName("request_id")] string RequestId,
        [property: JsonPropertyName("value")] JsonValue? Value,
        [property: JsonPropertyName("verification_status")] JsonValue? VerificationStatus,
        [property: JsonPropertyName("class")] string? ErrorClass,
        [property: JsonPropertyName("protocol_result_code")] int? ProtocolResultCode,
        [property: JsonPropertyName("retriability")] string? Retriability,
        [property: JsonPropertyName("reason")] string? Reason);
}

public sealed class HumanHttpTransport : IPlatformTransport
{
    private const int MaximumResponseBytes = 8 * 1024 * 1024;
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private readonly Uri _baseUri;
    private readonly HttpClient _httpClient;
    private readonly AccessToken? _accessToken;

    public HumanHttpTransport(Uri baseUri, HttpClient? httpClient = null, AccessToken? accessToken = null)
    {
        if (!baseUri.IsAbsoluteUri || !string.IsNullOrEmpty(baseUri.UserInfo) || string.IsNullOrEmpty(baseUri.Host) ||
            (baseUri.Scheme != Uri.UriSchemeHttps && (baseUri.Scheme != Uri.UriSchemeHttp || !IsLoopback(baseUri.Host))))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _baseUri = baseUri;
        _httpClient = httpClient ?? new HttpClient();
        _accessToken = accessToken;
    }

    public async Task<JsonValue> SendAsync(TransportCall call, CancellationToken cancellationToken = default)
    {
        var descriptor = call.Operation.Descriptor();
        if (descriptor.Plane != PlatformPlane.Human)
            throw new PlatformSdkException(SdkErrorCode.UnavailableCapability, RetryClass.Never);
        var path = ResolvePath(descriptor.Path, call.PathParameters);
        var target = new Uri(_baseUri, path);
        if (target.Scheme != _baseUri.Scheme || target.Host != _baseUri.Host || target.Port != _baseUri.Port)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);

        using var request = new HttpRequestMessage(ToHttpMethod(descriptor.Method), target);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        request.Headers.UserAgent.ParseAdd("layerx-dotnet/0.1.0");
        if (!descriptor.Bodyless)
        {
            var encoded = JsonSerializer.SerializeToUtf8Bytes(call.Request, JsonOptions);
            request.Content = new ByteArrayContent(encoded);
            request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        }
        if (call.IdempotencyKey is { } key)
            request.Headers.TryAddWithoutValidation("Idempotency-Key", key.Value);
        _accessToken?.Authorize(request);

        using var response = await _httpClient.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken).ConfigureAwait(false);
        var encodedResponse = await ReadBoundedAsync(response.Content, cancellationToken).ConfigureAwait(false);
        HumanEnvelope? envelope;
        try { envelope = JsonSerializer.Deserialize<HumanEnvelope>(encodedResponse, JsonOptions); }
        catch (JsonException) { throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never); }
        if (envelope is null || string.IsNullOrEmpty(envelope.Trace) || Encoding.UTF8.GetByteCount(envelope.Trace) > 512 ||
            envelope.Trace.Contains('\0') || envelope.Trace.Contains('\r') || envelope.Trace.Contains('\n'))
            throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never);
        if (envelope.Ok)
        {
            if (!response.IsSuccessStatusCode || envelope.Error is not null || envelope.Result is null)
                throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never);
            return envelope.Result;
        }
        if (response.IsSuccessStatusCode || envelope.Error is null || envelope.Result is not null)
            throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never);
        throw envelope.Error.ToSdkException(envelope.Trace);
    }

    private static async Task<byte[]> ReadBoundedAsync(HttpContent content, CancellationToken cancellationToken)
    {
        await using var stream = await content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
        using var output = new MemoryStream();
        var buffer = new byte[16 * 1024];
        while (true)
        {
            var count = await stream.ReadAsync(buffer.AsMemory(), cancellationToken).ConfigureAwait(false);
            if (count == 0) return output.ToArray();
            if (output.Length + count > MaximumResponseBytes)
                throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never);
            output.Write(buffer, 0, count);
        }
    }

    private static string ResolvePath(string template, IReadOnlyDictionary<string, string> parameters)
    {
        var path = template;
        foreach (var (name, value) in parameters)
        {
            var token = "{" + name + "}";
            if (string.IsNullOrEmpty(name) || string.IsNullOrEmpty(value) || !path.Contains(token, StringComparison.Ordinal))
                throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
            path = path.Replace(token, Uri.EscapeDataString(value), StringComparison.Ordinal);
        }
        if (!path.StartsWith("/", StringComparison.Ordinal) || path.Contains('{') || path.Contains('}'))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        return path;
    }

    private static HttpMethod ToHttpMethod(SdkHttpMethod method) => method switch
    {
        SdkHttpMethod.Get => HttpMethod.Get,
        SdkHttpMethod.Post => HttpMethod.Post,
        SdkHttpMethod.Put => HttpMethod.Put,
        SdkHttpMethod.Patch => HttpMethod.Patch,
        SdkHttpMethod.Delete => HttpMethod.Delete,
        _ => throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never),
    };

    private static bool IsLoopback(string host)
    {
        if (string.Equals(host, "localhost", StringComparison.OrdinalIgnoreCase)) return true;
        return IPAddress.TryParse(host, out var address) && IPAddress.IsLoopback(address);
    }

    private sealed record HumanEnvelope(
        [property: JsonPropertyName("ok")] bool Ok,
        [property: JsonPropertyName("result")] JsonValue? Result,
        [property: JsonPropertyName("error")] HumanApiError? Error,
        [property: JsonPropertyName("trace")] string Trace);

    private sealed record HumanApiError(
        [property: JsonPropertyName("code")] string Code,
        [property: JsonPropertyName("retry")] string Retry,
        [property: JsonPropertyName("retry_after_ms")] ulong? RetryAfterMilliseconds)
    {
        public PlatformSdkException ToSdkException(string trace)
        {
            var code = Code switch
            {
                "rate-limited" => SdkErrorCode.RateLimit,
                "unavailable" or "upstream-degraded" => SdkErrorCode.TransportFailure,
                "refused-by-policy" => SdkErrorCode.PolicyRefusal,
                "refused-by-budget" or "refused-by-limit" => SdkErrorCode.BudgetRefusal,
                "refused-by-capability" or "forbidden" or "unauthenticated" or "session-expired" or "step-up-required" => SdkErrorCode.CapabilityRefusal,
                "conflict" => SdkErrorCode.IdempotencyConflict,
                _ => SdkErrorCode.CoreRejection,
            };
            var retry = Retry switch
            {
                "retriable" => RetryClass.Safe,
                "retriable-after" when RetryAfterMilliseconds is not null => RetryClass.After,
                "structural" or "final" => RetryClass.Never,
                _ => throw new PlatformSdkException(SdkErrorCode.DecodeFailure, RetryClass.Never),
            };
            return new PlatformSdkException(code, retry, trace, retryAfterMilliseconds: RetryAfterMilliseconds);
        }
    }
}
