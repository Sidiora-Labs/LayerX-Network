package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.CompletionStage;

/** HTTP/JSON transport for the schema-defined human routes and the agent RPC endpoint. */
public final class HttpProductionTransport implements ProductionTransport {
    private static final int MAXIMUM_RESPONSE_BYTES = 8 * 1024 * 1024;
    @FunctionalInterface public interface Credential { void apply(HttpRequest.Builder request); }

    public static final class BearerCredential implements Credential, AutoCloseable {
        private final SecretBytes token;
        public BearerCredential(SecretBytes token) { this.token = Objects.requireNonNull(token, "token"); }
        @Override public void apply(HttpRequest.Builder request) {
            token.use(bytes -> {
                request.header("Authorization", "Bearer " + new String(bytes, StandardCharsets.UTF_8));
                return null;
            });
        }
        @Override public void close() { token.close(); }
        @Override public String toString() { return "[REDACTED]"; }
    }

    private final HttpClient client;
    private final ObjectMapper mapper;
    private final URI humanBaseUri;
    private final URI agentEndpoint;
    private final Duration timeout;
    private final Credential credential;

    public HttpProductionTransport(HttpClient client, ObjectMapper mapper, URI humanBaseUri,
                                   URI agentEndpoint, Duration timeout, Credential credential) {
        this.client = Objects.requireNonNull(client, "client");
        this.mapper = Objects.requireNonNull(mapper, "mapper");
        this.humanBaseUri = requireHttpUri(humanBaseUri);
        this.agentEndpoint = requireHttpUri(agentEndpoint);
        this.timeout = Objects.requireNonNull(timeout, "timeout");
        if (timeout.isZero() || timeout.isNegative()) throw PlatformSdkException.invalidArgument();
        this.credential = credential;
    }

    public static HttpProductionTransport create(URI humanBaseUri, URI agentEndpoint, Credential credential) {
        return new HttpProductionTransport(HttpClient.newBuilder().version(HttpClient.Version.HTTP_2).build(),
            new ObjectMapper(), humanBaseUri, agentEndpoint, Duration.ofSeconds(30), credential);
    }

    @Override
    public <T> CompletionStage<T> call(Call call, JavaType responseType) {
        Objects.requireNonNull(call, "call");
        Objects.requireNonNull(responseType, "responseType");
        HttpRequest request;
        try {
            request = call.operation().plane() == OperationCatalog.Plane.HUMAN ? humanRequest(call) : agentRequest(call);
        } catch (IOException error) {
            return CompletableFuture.failedFuture(new PlatformSdkException(
                PlatformSdkException.Code.INVALID_ARGUMENT, PlatformSdkException.Retry.NEVER, null, null, null));
        }
        return client.sendAsync(request, HttpResponse.BodyHandlers.ofInputStream())
            .handle((response, failure) -> {
                if (failure != null) throw new CompletionException(new PlatformSdkException(
                    PlatformSdkException.Code.TRANSPORT_FAILURE, PlatformSdkException.Retry.SAFE, null, null, null));
                try (var body = response.body()) {
                    byte[] encoded = body.readNBytes(MAXIMUM_RESPONSE_BYTES + 1);
                    if (encoded.length > MAXIMUM_RESPONSE_BYTES) throw new PlatformSdkException(
                        PlatformSdkException.Code.DECODE_FAILURE, PlatformSdkException.Retry.NEVER, null, null, null);
                    return decode(response.statusCode(), encoded, responseType, call.operation().plane());
                } catch (IOException error) {
                    throw new CompletionException(new PlatformSdkException(PlatformSdkException.Code.TRANSPORT_FAILURE,
                        PlatformSdkException.Retry.SAFE, null, null, null));
                }
            });
    }

    private HttpRequest humanRequest(Call call) throws IOException {
        var route = OperationCatalog.HUMAN_ROUTES.get(call.operation().wireName());
        if (route == null) throw PlatformSdkException.invalidArgument();
        String path = route.path();
        for (String parameter : route.pathParameters()) {
            String value = call.pathParameters().require(parameter);
            path = path.replace("{" + parameter + "}", encodePath(value));
        }
        var builder = common(endpoint(humanBaseUri, path), call);
        if (route.bodyless()) builder.method(route.method(), HttpRequest.BodyPublishers.noBody());
        else builder.method(route.method(), jsonBody(call.request()));
        return builder.build();
    }

    private HttpRequest agentRequest(Call call) throws IOException {
        var body = mapper.createObjectNode();
        body.put("operation", call.operation().wireName());
        body.set("request", call.request());
        return common(agentEndpoint, call).POST(jsonBody(body)).build();
    }

    private HttpRequest.Builder common(URI uri, Call call) {
        var builder = HttpRequest.newBuilder(uri).timeout(timeout)
            .header("Accept", "application/json").header("Content-Type", "application/json")
            .header("User-Agent", "layerx-jvm/0.1.0");
        if (call.idempotencyKey() != null) builder.header("Idempotency-Key", call.idempotencyKey().value());
        if (credential != null) credential.apply(builder);
        return builder;
    }

    private HttpRequest.BodyPublisher jsonBody(Object value) throws IOException {
        return HttpRequest.BodyPublishers.ofByteArray(mapper.writeValueAsBytes(value == null ? Map.of() : value));
    }

    private <T> T decode(int status, byte[] encoded, JavaType type, OperationCatalog.Plane plane) {
        try {
            JsonNode envelope = mapper.readTree(encoded);
            if (envelope == null || !envelope.isObject()) {
                throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                    PlatformSdkException.Retry.NEVER, null, null, null);
            }
            if (plane == OperationCatalog.Plane.AGENT) return decodeAgent(status, envelope, type);
            String trace = envelope.path("trace").isTextual() ? envelope.path("trace").textValue() : null;
            if (!envelope.path("ok").isBoolean()) throw decodeFailure(trace);
            if (!envelope.path("ok").booleanValue()) throw serviceError(status, trace, envelope.path("error"), plane);
            if (status < 200 || status >= 300 || envelope.has("error")) throw new PlatformSdkException(
                PlatformSdkException.Code.DECODE_FAILURE, PlatformSdkException.Retry.NEVER, trace, null, null);
            if (!envelope.has("result")) throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, trace, null, null);
            JsonNode result = envelope.get("result");
            if (result instanceof com.fasterxml.jackson.databind.node.ObjectNode object) {
                result = SchemaTypes.canonicalBody(object);
            }
            return mapper.convertValue(result, type);
        } catch (PlatformSdkException error) {
            throw error;
        } catch (IOException | IllegalArgumentException error) {
            throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, null, null, null);
        }
    }

    private <T> T decodeAgent(int status, JsonNode envelope, JavaType type) {
        if (envelope.has("class")) throw agentServiceError(envelope);
        String requestId = envelope.path("request_id").asText("");
        if (status < 200 || status >= 300 || requestId.isEmpty() || !envelope.has("value")
                || !envelope.path("verification_status").isObject()) throw decodeFailure(requestId);
        JsonNode value = envelope.get("value");
        if (value instanceof com.fasterxml.jackson.databind.node.ObjectNode object) {
            value = SchemaTypes.canonicalBody(object);
        }
        return mapper.convertValue(value, type);
    }

    private static PlatformSdkException agentServiceError(JsonNode error) {
        try {
            var exactClass = SchemaErrors.AgentClass.fromWire(error.path("class").asText(null));
            var exactRetry = SchemaErrors.AgentRetriability.fromWire(error.path("retriability").asText(null));
            String requestId = error.path("request_id").asText("");
            if (requestId.isEmpty() || !error.path("reason").isTextual()) throw new IllegalArgumentException();
            JsonNode protocolResult = error.path("protocol_result_code");
            if (!protocolResult.isNull() && !protocolResult.canConvertToInt()) throw new IllegalArgumentException();
            Integer resultCode = protocolResult.isNull() ? null : protocolResult.intValue();
            PlatformSdkException.Retry retry = exactRetry == SchemaErrors.AgentRetriability.RETRIABLE
                ? PlatformSdkException.Retry.SAFE : PlatformSdkException.Retry.NEVER;
            return PlatformSdkException.agent(mapAgentClass(exactClass), retry, requestId, resultCode, null,
                exactClass, exactRetry);
        } catch (IllegalArgumentException invalidSchemaError) {
            throw decodeFailure(null);
        }
    }

    private static PlatformSdkException.Code mapAgentClass(SchemaErrors.AgentClass value) {
        return switch (value) {
            case TRANSPORT_FAILURE -> PlatformSdkException.Code.TRANSPORT_FAILURE;
            case DEADLINE -> PlatformSdkException.Code.DEADLINE;
            case PROTOCOL_INCOMPATIBILITY -> PlatformSdkException.Code.PROTOCOL_INCOMPATIBILITY;
            case UNAVAILABLE_CAPABILITY -> PlatformSdkException.Code.UNAVAILABLE_CAPABILITY;
            case CORE_REJECTION -> PlatformSdkException.Code.CORE_REJECTION;
            case VERIFICATION_FAILURE -> PlatformSdkException.Code.VERIFICATION_FAILURE;
            case POLICY_REFUSAL -> PlatformSdkException.Code.POLICY_REFUSAL;
            case CAPABILITY_REFUSAL -> PlatformSdkException.Code.CAPABILITY_REFUSAL;
            case BUDGET_REFUSAL -> PlatformSdkException.Code.BUDGET_REFUSAL;
            case RATE_LIMIT -> PlatformSdkException.Code.RATE_LIMIT;
            case IDEMPOTENCY_CONFLICT -> PlatformSdkException.Code.IDEMPOTENCY_CONFLICT;
            case INTERNAL_FAULT -> PlatformSdkException.Code.INTERNAL_FAULT;
        };
    }

    private static PlatformSdkException serviceError(int status, String trace, JsonNode error,
                                                     OperationCatalog.Plane plane) {
        String code = error.path("code").asText("");
        var mapped = switch (code) {
            case "TransportFailure" -> PlatformSdkException.Code.TRANSPORT_FAILURE;
            case "Deadline" -> PlatformSdkException.Code.DEADLINE;
            case "ProtocolIncompatibility" -> PlatformSdkException.Code.PROTOCOL_INCOMPATIBILITY;
            case "UnavailableCapability" -> PlatformSdkException.Code.UNAVAILABLE_CAPABILITY;
            case "CoreRejection" -> PlatformSdkException.Code.CORE_REJECTION;
            case "VerificationFailure" -> PlatformSdkException.Code.VERIFICATION_FAILURE;
            case "PolicyRefusal" -> PlatformSdkException.Code.POLICY_REFUSAL;
            case "CapabilityRefusal" -> PlatformSdkException.Code.CAPABILITY_REFUSAL;
            case "BudgetRefusal" -> PlatformSdkException.Code.BUDGET_REFUSAL;
            case "RateLimit" -> PlatformSdkException.Code.RATE_LIMIT;
            case "IdempotencyConflict" -> PlatformSdkException.Code.IDEMPOTENCY_CONFLICT;
            case "InternalFault" -> PlatformSdkException.Code.INTERNAL_FAULT;
            case "rate-limited" -> PlatformSdkException.Code.RATE_LIMIT;
            case "conflict" -> PlatformSdkException.Code.IDEMPOTENCY_CONFLICT;
            case "refused-by-policy" -> PlatformSdkException.Code.POLICY_REFUSAL;
            case "refused-by-budget" -> PlatformSdkException.Code.BUDGET_REFUSAL;
            case "refused-by-capability", "forbidden" -> PlatformSdkException.Code.CAPABILITY_REFUSAL;
            case "refused-by-protocol", "refused-by-limit" -> PlatformSdkException.Code.CORE_REJECTION;
            case "unavailable", "upstream-degraded" -> PlatformSdkException.Code.UNAVAILABLE_CAPABILITY;
            default -> status >= 500 ? PlatformSdkException.Code.INTERNAL_FAULT : PlatformSdkException.Code.CORE_REJECTION;
        };
        Long after = error.path("retry_after_ms").canConvertToLong() ? error.path("retry_after_ms").longValue() : null;
        Integer resultCode = error.path("protocol_result_code").canConvertToInt()
            ? error.path("protocol_result_code").intValue() : null;
        try {
            if (plane == OperationCatalog.Plane.HUMAN) {
                var exactCode = SchemaErrors.HumanCode.fromWire(code);
                var exactRetry = SchemaErrors.HumanRetriability.fromWire(error.path("retry").asText(null));
                var retry = switch (exactRetry) {
                    case RETRIABLE -> PlatformSdkException.Retry.SAFE;
                    case RETRIABLE_AFTER -> PlatformSdkException.Retry.AFTER;
                    case STRUCTURAL, FINAL -> PlatformSdkException.Retry.NEVER;
                };
                if (exactRetry == SchemaErrors.HumanRetriability.RETRIABLE_AFTER && after == null) {
                    throw new IllegalArgumentException("missing retry_after_ms");
                }
                return PlatformSdkException.human(mapped, retry, trace, resultCode, after, exactCode, exactRetry);
            }
            var exactClass = SchemaErrors.AgentClass.fromWire(code);
            var exactRetry = SchemaErrors.AgentRetriability.fromWire(error.path("retry").asText(null));
            var retry = exactRetry == SchemaErrors.AgentRetriability.RETRIABLE
                ? PlatformSdkException.Retry.SAFE : PlatformSdkException.Retry.NEVER;
            return PlatformSdkException.agent(mapped, retry, trace, resultCode, after, exactClass, exactRetry);
        } catch (IllegalArgumentException invalidSchemaError) {
            throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, trace, null, null);
        }
    }

    private static URI requireHttpUri(URI uri) {
        Objects.requireNonNull(uri, "uri");
        if (!("http".equalsIgnoreCase(uri.getScheme()) || "https".equalsIgnoreCase(uri.getScheme()))) {
            throw PlatformSdkException.invalidArgument();
        }
        if (uri.getHost() == null || uri.getUserInfo() != null || uri.getQuery() != null || uri.getFragment() != null) {
            throw PlatformSdkException.invalidArgument();
        }
        if ("http".equalsIgnoreCase(uri.getScheme()) && !isLoopback(uri.getHost())) {
            throw PlatformSdkException.invalidArgument();
        }
        return uri;
    }
    private static boolean isLoopback(String host) {
        String normalized = host.toLowerCase(java.util.Locale.ROOT);
        return normalized.equals("localhost") || normalized.equals("::1") || normalized.equals("[::1]")
            || normalized.startsWith("127.");
    }
    private static URI endpoint(URI base, String path) {
        String prefix = base.toString();
        while (prefix.endsWith("/")) prefix = prefix.substring(0, prefix.length() - 1);
        return URI.create(prefix + path);
    }
    private static String encodePath(String value) { return URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20"); }
    private static PlatformSdkException decodeFailure(String requestId) {
        return new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
            PlatformSdkException.Retry.NEVER, requestId, null, null);
    }
}
