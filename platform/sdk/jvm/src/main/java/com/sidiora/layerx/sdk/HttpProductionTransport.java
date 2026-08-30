package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.math.BigInteger;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.CompletionStage;

/** HTTP/JSON transport for the schema-defined human and Agent routes. */
public final class HttpProductionTransport implements ProductionTransport {
    private static final int MAXIMUM_RESPONSE_BYTES = 8 * 1024 * 1024;
    private static final int MAXIMUM_PROGRAMS_REQUEST_BYTES = 8 * 1024 * 1024;
    private static final int MAXIMUM_PROGRAM_BYTES = 1_048_576;
    private record ProgramRoute(String method, String path, List<String> pathParameters,
                                boolean idempotency) {}
    private static final Map<String, ProgramRoute> PROGRAM_ROUTES = Map.of(
        "program.discover", new ProgramRoute("GET", "/v1/programs/registry/{program_id}",
            List.of("program_id"), false),
        "program.interface", new ProgramRoute("GET", "/v1/programs/registry/{program_id}/interface",
            List.of("program_id"), false),
        "program.simulate", new ProgramRoute("POST", "/v1/programs/simulate", List.of(), false),
        "program.call", new ProgramRoute("POST", "/v1/programs/call", List.of(), true),
        "program.receipt", new ProgramRoute("GET", "/v1/programs/receipts/by-idempotency/{idempotency_key}",
            List.of("idempotency_key"), false),
        "program.activity", new ProgramRoute("GET", "/v1/programs/activities/{activity_id}",
            List.of("activity_id"), false));
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

    public static final class LayerXKeyCredential implements Credential, AutoCloseable {
        private final String keyId;
        private final SecretBytes secret;
        public LayerXKeyCredential(String keyId, SecretBytes secret) {
            if (!validLayerXKeyId(keyId)) throw PlatformSdkException.invalidArgument();
            this.keyId = keyId;
            this.secret = Objects.requireNonNull(secret, "secret");
            secret.use(bytes -> {
                if (!validLayerXKeySecret(new String(bytes, StandardCharsets.US_ASCII))) {
                    throw PlatformSdkException.invalidArgument();
                }
                return null;
            });
        }
        @Override public void apply(HttpRequest.Builder request) {
            secret.use(bytes -> {
                request.header("Authorization", "LayerX-Key " + keyId + ":"
                    + new String(bytes, StandardCharsets.US_ASCII));
                return null;
            });
        }
        @Override public void close() { secret.close(); }
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
        if (client.followRedirects() != HttpClient.Redirect.NEVER) throw PlatformSdkException.invalidArgument();
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

    @Override
    public <T> CompletionStage<T> callPrograms(ProgramsCall call, JavaType responseType) {
        Objects.requireNonNull(call, "call");
        Objects.requireNonNull(responseType, "responseType");
        final HttpRequest request;
        try {
            request = programRequest(call);
        } catch (IOException error) {
            return CompletableFuture.failedFuture(PlatformSdkException.invalidArgument());
        } catch (PlatformSdkException error) {
            return CompletableFuture.failedFuture(error);
        }
        final CompletableFuture<HttpResponse<java.io.InputStream>> pending;
        try {
            pending = client.sendAsync(request, HttpResponse.BodyHandlers.ofInputStream());
        } catch (RuntimeException error) {
            return CompletableFuture.failedFuture(programTransportFailure(call.operation()));
        }
        return pending.handle((response, failure) -> {
            if (failure != null) throw new CompletionException(programTransportFailure(call.operation()));
            try (var body = response.body()) {
                if (!jsonContentType(response)) throw programDecodeFailure(call.operation(), null);
                byte[] encoded = body.readNBytes(MAXIMUM_RESPONSE_BYTES + 1);
                if (encoded.length > MAXIMUM_RESPONSE_BYTES) throw programDecodeFailure(call.operation(), null);
                return decodePrograms(call.operation(), response.statusCode(), encoded, responseType);
            } catch (IOException error) {
                throw new CompletionException(programTransportFailure(call.operation()));
            } catch (PlatformSdkException error) {
                if ("program.call".equals(call.operation())
                        && error.code() == PlatformSdkException.Code.DECODE_FAILURE) {
                    throw new CompletionException(unknownOutcome());
                }
                throw error;
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

    HttpRequest programRequest(ProgramsCall call) throws IOException {
        ProgramRoute route = PROGRAM_ROUTES.get(call.operation());
        if (route == null || !call.pathParameters().values().keySet().equals(Set.copyOf(route.pathParameters()))) {
            throw PlatformSdkException.invalidArgument();
        }
        if (route.idempotency()) {
            if (call.idempotencyKey() == null || !canonicalLowerHex(call.idempotencyKey().value(), 32)) {
                throw new PlatformSdkException(PlatformSdkException.Code.IDEMPOTENCY_REQUIRED,
                    PlatformSdkException.Retry.NEVER, null, null, null);
            }
        } else if (call.idempotencyKey() != null) {
            throw PlatformSdkException.invalidArgument();
        }
        validateProgramRequest(call);
        byte[] body = mapper.writeValueAsBytes(call.request());
        if (body.length == 0 || body.length > MAXIMUM_PROGRAMS_REQUEST_BYTES) {
            throw PlatformSdkException.invalidArgument();
        }
        String path = route.path();
        for (String parameter : route.pathParameters()) {
            String value = call.pathParameters().require(parameter);
            JsonNode bodyValue = call.request().get(parameter);
            if (!canonicalLowerHex(value, 32) || bodyValue == null || !bodyValue.isTextual()
                    || !value.equals(bodyValue.textValue())) throw PlatformSdkException.invalidArgument();
            path = path.replace("{" + parameter + "}", encodePath(value));
        }
        var builder = HttpRequest.newBuilder(rootEndpoint(agentEndpoint, path)).timeout(timeout)
            .header("Accept", "application/json").header("Content-Type", "application/json")
            .header("User-Agent", "layerx-jvm/0.1.0");
        if (route.idempotency()) builder.header("Idempotency-Key", call.idempotencyKey().value());
        if (credential != null) credential.apply(builder);
        HttpRequest request = builder.method(route.method(), HttpRequest.BodyPublishers.ofByteArray(body)).build();
        List<String> authorization = request.headers().allValues("Authorization");
        if (authorization.size() != 1 || !validLayerXAuthorization(authorization.get(0))) {
            throw new PlatformSdkException(PlatformSdkException.Code.CAPABILITY_REFUSAL,
                PlatformSdkException.Retry.NEVER, null, null, null);
        }
        return request;
    }

    private static void validateProgramRequest(ProgramsCall call) {
        JsonNode request = call.request();
        switch (call.operation()) {
            case "program.discover", "program.interface" -> {
                if (!exactFields(request, "program_id", "requested_verification_level")
                        || !canonicalProgram(request.get("program_id"))
                        || !"sequencer-signed".equals(request.path("requested_verification_level").textValue())) {
                    throw PlatformSdkException.invalidArgument();
                }
            }
            case "program.receipt" -> {
                if (!exactFields(request, "idempotency_key", "expected_activity_id",
                        "requested_verification_level")
                        || !canonicalHexNode(request.get("idempotency_key"), 32, false)
                        || !canonicalHexNode(request.get("expected_activity_id"), 32, false)
                        || !"sequencer-signed".equals(request.path("requested_verification_level").textValue())) {
                    throw PlatformSdkException.invalidArgument();
                }
            }
            case "program.activity" -> {
                if (!exactFields(request, "activity_id", "requested_verification_level")
                        || !canonicalHexNode(request.get("activity_id"), 32, false)
                        || !"sequencer-signed".equals(request.path("requested_verification_level").textValue())) {
                    throw PlatformSdkException.invalidArgument();
                }
            }
            case "program.simulate", "program.call" -> validateProgramCall(request);
            default -> throw PlatformSdkException.invalidArgument();
        }
    }

    private static void validateProgramCall(JsonNode request) {
        if (!exactFields(request, "program_id", "calldata", "budget", "capabilities", "signed_activity")
                || !canonicalProgram(request.get("program_id"))
                || !canonicalBoundedHex(request.get("calldata"), MAXIMUM_PROGRAM_BYTES, true)
                || !canonicalBoundedHex(request.get("signed_activity"), MAXIMUM_PROGRAM_BYTES, false)) {
            throw PlatformSdkException.invalidArgument();
        }
        JsonNode budget = request.get("budget");
        if (!exactFields(budget, "fuel", "fee_limit")
                || !canonicalUnsigned(budget.get("fuel"), 64, true)
                || !canonicalUnsigned(budget.get("fee_limit"), 128, false)) {
            throw PlatformSdkException.invalidArgument();
        }
        JsonNode capabilities = request.get("capabilities");
        List<String> order = List.of("storage_read", "storage_write", "transfer", "emit_event", "compose");
        if (capabilities == null || !capabilities.isArray() || capabilities.size() > order.size()) {
            throw PlatformSdkException.invalidArgument();
        }
        int previous = -1;
        for (JsonNode capability : capabilities) {
            int current = capability.isTextual() ? order.indexOf(capability.textValue()) : -1;
            if (current <= previous) throw PlatformSdkException.invalidArgument();
            previous = current;
        }
    }

    private static boolean canonicalProgram(JsonNode value) {
        return canonicalHexNode(value, 32, false) && !"0".repeat(64).equals(value.textValue());
    }

    private static boolean canonicalHexNode(JsonNode value, int bytes, boolean emptyAllowed) {
        return value != null && value.isTextual()
            && (emptyAllowed && value.textValue().isEmpty() || canonicalLowerHex(value.textValue(), bytes));
    }

    private static boolean canonicalBoundedHex(JsonNode value, int maximumBytes, boolean emptyAllowed) {
        if (value == null || !value.isTextual()) return false;
        String text = value.textValue();
        return text.length() <= maximumBytes * 2 && (text.length() & 1) == 0
            && (emptyAllowed || !text.isEmpty()) && canonicalLowerHex(text, text.length() / 2);
    }

    private static boolean canonicalUnsigned(JsonNode value, int bits, boolean positive) {
        if (value == null || !value.isTextual()) return false;
        String text = value.textValue();
        if (text.isEmpty() || text.length() > 1 && text.charAt(0) == '0'
                || !text.chars().allMatch(current -> current >= '0' && current <= '9')) return false;
        try {
            BigInteger integer = new BigInteger(text);
            return integer.bitLength() <= bits && (!positive || integer.signum() > 0);
        } catch (NumberFormatException error) {
            return false;
        }
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

    <T> T decodePrograms(String operation, int status, byte[] encoded, JavaType type) {
        try {
            JsonNode envelope = mapper.readTree(encoded);
            if (envelope == null || !envelope.isObject()) throw decodeFailure(null);
            if (envelope.has("class")) {
                if (status >= 200 && status < 300 || !exactFields(envelope,
                        "class", "protocol_result_code", "retriability", "request_id", "reason")) {
                    throw decodeFailure(null);
                }
                throw programAgentServiceError(envelope);
            }
            if (status < 200 || status >= 300 || !exactFields(envelope,
                    "request_id", "value", "verification_status")) throw decodeFailure(null);
            String requestId = boundedTrace(envelope.get("request_id"));
            JsonNode value = envelope.get("value");
            if (value.isNull() || !validProgramVerification(operation, value,
                    envelope.get("verification_status"))) throw decodeFailure(requestId);
            return mapper.convertValue(value, type);
        } catch (PlatformSdkException error) {
            throw error;
        } catch (IOException | IllegalArgumentException error) {
            throw decodeFailure(null);
        }
    }

    private static boolean validProgramVerification(String operation, JsonNode value, JsonNode verification) {
        if (Set.of("program.discover", "program.interface").contains(operation)) {
            return exactFields(verification, "state", "requested", "achieved", "reason")
                && "Unverified".equals(verification.path("state").textValue())
                && "SequencerSigned".equals(verification.path("requested").textValue())
                && "Unverified".equals(verification.path("achieved").textValue())
                && "server_side_receipt_verification_only".equals(
                    verification.path("reason").textValue());
        }
        boolean pending = Set.of("program.call", "program.receipt", "program.activity").contains(operation)
            && value.isObject() && Set.of("unknown", "pending").contains(value.path("state").textValue());
        if (pending) {
            return exactFields(verification, "state", "requested", "achieved", "reason")
                && "Unverified".equals(verification.path("state").textValue())
                && "SequencerSigned".equals(verification.path("requested").textValue())
                && "Unverified".equals(verification.path("achieved").textValue())
                && "receipt_pending".equals(verification.path("reason").textValue());
        }
        return Set.of("program.simulate", "program.call", "program.receipt", "program.activity")
                .contains(operation)
            && exactFields(verification, "state", "level")
            && "Achieved".equals(verification.path("state").textValue())
            && "SequencerSigned".equals(verification.path("level").textValue());
    }

    private static PlatformSdkException programAgentServiceError(JsonNode error) {
        try {
            var exactClass = SchemaErrors.AgentClass.fromWire(requiredText(error.get("class")));
            var exactRetry = SchemaErrors.AgentRetriability.fromWire(requiredText(error.get("retriability")));
            String requestId = boundedTrace(error.get("request_id"));
            String reason = requiredText(error.get("reason"));
            if (reason.length() > 256 || !reason.chars().allMatch(value -> value >= 'a' && value <= 'z'
                    || value >= '0' && value <= '9' || value == '_' || value == '.' || value == '/')) {
                throw new IllegalArgumentException();
            }
            JsonNode protocolResult = error.get("protocol_result_code");
            if (protocolResult == null || !protocolResult.isNull() && !protocolResult.canConvertToInt()) {
                throw new IllegalArgumentException();
            }
            Integer resultCode = protocolResult.isNull() ? null : protocolResult.intValue();
            PlatformSdkException.Retry retry = exactRetry == SchemaErrors.AgentRetriability.RETRIABLE
                ? PlatformSdkException.Retry.SAFE : PlatformSdkException.Retry.NEVER;
            return PlatformSdkException.agent(mapAgentClass(exactClass), retry, requestId, resultCode, null,
                exactClass, exactRetry);
        } catch (IllegalArgumentException invalidSchemaError) {
            throw decodeFailure(null);
        }
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
    private static URI rootEndpoint(URI base, String path) {
        return URI.create(base.getScheme() + "://" + base.getRawAuthority() + path);
    }
    private static String encodePath(String value) { return URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20"); }
    private static boolean canonicalLowerHex(String value, int bytes) {
        if (value == null || value.length() != bytes * 2) return false;
        for (int index = 0; index < value.length(); index++) {
            char current = value.charAt(index);
            if (!(current >= '0' && current <= '9' || current >= 'a' && current <= 'f')) return false;
        }
        return true;
    }
    private static boolean validLayerXKeyId(String value) {
        if (value == null || value.isEmpty() || value.length() > 64) return false;
        return value.chars().allMatch(current -> current >= 'a' && current <= 'z'
            || current >= 'A' && current <= 'Z' || current >= '0' && current <= '9'
            || current == '-' || current == '_');
    }
    private static boolean validLayerXKeySecret(String value) {
        return value != null && value.startsWith("lxp_live_")
            && canonicalLowerHex(value.substring("lxp_live_".length()), 32);
    }
    private static boolean validLayerXAuthorization(String value) {
        if (value == null || !value.startsWith("LayerX-Key ")) return false;
        int separator = value.indexOf(':', "LayerX-Key ".length());
        return separator > "LayerX-Key ".length()
            && value.indexOf(':', separator + 1) < 0
            && validLayerXKeyId(value.substring("LayerX-Key ".length(), separator))
            && validLayerXKeySecret(value.substring(separator + 1));
    }
    private static boolean jsonContentType(HttpResponse<?> response) {
        String value = response.headers().firstValue("Content-Type").orElse("");
        int separator = value.indexOf(';');
        String mediaType = separator < 0 ? value : value.substring(0, separator);
        return "application/json".equalsIgnoreCase(mediaType.trim());
    }
    private static boolean exactFields(JsonNode value, String... expected) {
        if (value == null || !value.isObject() || value.size() != expected.length) return false;
        for (String name : expected) if (!value.has(name)) return false;
        return true;
    }
    private static String requiredText(JsonNode value) {
        if (value == null || !value.isTextual() || value.textValue().isEmpty()) {
            throw new IllegalArgumentException();
        }
        return value.textValue();
    }
    private static String boundedTrace(JsonNode value) {
        String trace = requiredText(value);
        if (trace.length() > 256 || !trace.chars().allMatch(current -> current >= 0x21 && current <= 0x7e)) {
            throw new IllegalArgumentException();
        }
        return trace;
    }
    private static PlatformSdkException programTransportFailure(String operation) {
        return "program.call".equals(operation) ? unknownOutcome() : new PlatformSdkException(
            PlatformSdkException.Code.TRANSPORT_FAILURE, PlatformSdkException.Retry.SAFE, null, null, null);
    }
    private static PlatformSdkException programDecodeFailure(String operation, String requestId) {
        return new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
            PlatformSdkException.Retry.NEVER, requestId, null, null);
    }
    private static PlatformSdkException unknownOutcome() {
        return new PlatformSdkException(PlatformSdkException.Code.UNKNOWN_OUTCOME,
            PlatformSdkException.Retry.UNKNOWN_OUTCOME, null, null, null);
    }
    private static PlatformSdkException decodeFailure(String requestId) {
        return new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
            PlatformSdkException.Retry.NEVER, requestId, null, null);
    }
}
