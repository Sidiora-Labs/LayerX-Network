#nullable enable

using System.Net;
using System.Net.Http.Headers;
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
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private static readonly HashSet<string> Operations = new(StringComparer.Ordinal)
    {
        "program.discover", "program.interface", "program.simulate",
        "program.call", "program.receipt", "program.activity",
    };
    private readonly Uri _baseUri;
    private readonly HttpClient _httpClient;
    private readonly LayerXKeyCredential? _credential;

    public AgentHttpTransport(Uri baseUri, HttpClient? httpClient = null, LayerXKeyCredential? credential = null)
    {
        if (!baseUri.IsAbsoluteUri || !string.IsNullOrEmpty(baseUri.UserInfo) || string.IsNullOrEmpty(baseUri.Host) ||
            !string.IsNullOrEmpty(baseUri.Query) || !string.IsNullOrEmpty(baseUri.Fragment) ||
            (baseUri.Scheme != Uri.UriSchemeHttps && (baseUri.Scheme != Uri.UriSchemeHttp || !IsLoopback(baseUri.Host))))
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        _baseUri = baseUri;
        _httpClient = httpClient ?? new HttpClient();
        _credential = credential;
    }

    public async Task<JsonValue> SendAsync(TransportCall call, CancellationToken cancellationToken = default)
    {
        var descriptor = call.Operation.Descriptor();
        if (descriptor.Plane != PlatformPlane.Agent || !Operations.Contains(descriptor.Name))
            throw new PlatformSdkException(SdkErrorCode.UnavailableCapability, RetryClass.Never);
        if (descriptor.Name == "program.call")
        {
            if (call.IdempotencyKey is not { } key || !Hex32(key.Value))
                throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        }
        else if (call.IdempotencyKey is not null)
            throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never);
        var path = ResolvePath(descriptor.Path, call.PathParameters);
        var target = Endpoint(_baseUri, path);
        using var request = new HttpRequestMessage(ToHttpMethod(descriptor.Method), target);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        request.Headers.UserAgent.ParseAdd("layerx-dotnet/0.1.0");
        request.Content = new ByteArrayContent(JsonSerializer.SerializeToUtf8Bytes(call.Request, JsonOptions));
        request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        if (call.IdempotencyKey is { } idempotency)
            request.Headers.TryAddWithoutValidation("Idempotency-Key", idempotency.Value);
        _credential?.Authorize(request);
        using var response = await _httpClient.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken).ConfigureAwait(false);
        var encoded = await ReadBoundedAsync(response.Content, cancellationToken).ConfigureAwait(false);
        AgentEnvelope? envelope;
        try { envelope = JsonSerializer.Deserialize<AgentEnvelope>(encoded, JsonOptions); }
        catch (JsonException) { throw Decode(); }
        if (envelope is null) throw Decode();
        if (envelope.ErrorClass is not null)
        {
            if (response.IsSuccessStatusCode || envelope.Value is not null) throw Decode(envelope.RequestId);
            throw ServiceError(envelope);
        }
        if (!response.IsSuccessStatusCode || string.IsNullOrEmpty(envelope.RequestId) || envelope.Value is null)
            throw Decode(envelope.RequestId);
        if (!SequencerSigned(envelope.VerificationStatus))
            throw new PlatformSdkException(SdkErrorCode.VerificationFailure, RetryClass.Never, envelope.RequestId);
        return envelope.Value;
    }

    private static bool SequencerSigned(JsonValue? value) => value is JsonValue.ObjectValue map &&
        map.Value.TryGetValue("state", out var state) && state is JsonValue.StringValue { Value: "Achieved" } &&
        map.Value.TryGetValue("level", out var level) && level is JsonValue.StringValue { Value: "SequencerSigned" };

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

    private static HttpMethod ToHttpMethod(SdkHttpMethod method) => method switch
    {
        SdkHttpMethod.Get => HttpMethod.Get, SdkHttpMethod.Post => HttpMethod.Post,
        SdkHttpMethod.Put => HttpMethod.Put, SdkHttpMethod.Patch => HttpMethod.Patch,
        SdkHttpMethod.Delete => HttpMethod.Delete, _ => throw new PlatformSdkException(SdkErrorCode.InvalidArgument, RetryClass.Never),
    };

    private static bool IsLoopback(string host) => string.Equals(host, "localhost", StringComparison.OrdinalIgnoreCase) ||
        IPAddress.TryParse(host, out var address) && IPAddress.IsLoopback(address);
    private static bool Hex32(string value) => value.Length == 64 && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    private static PlatformSdkException Decode(string? requestId = null) => new(SdkErrorCode.DecodeFailure, RetryClass.Never, requestId);

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
