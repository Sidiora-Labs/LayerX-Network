package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.math.BigInteger;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;

/** Strongly typed schema boundary shared by the generated Agent and Human API catalogues. */
public final class SchemaTypes {
    private SchemaTypes() {}

    public sealed interface Operation permits AgentOperation, HumanOperation, TypedOperation {
        OperationCatalog.Plane plane();
        String wireName();
        boolean idempotencyRequired();
    }

    public interface GeneratedRequest {}
    public interface GeneratedResponse {}
    public interface GeneratedEvent {}

    public record TypedOperation<R extends GeneratedRequest, S extends GeneratedResponse>(
        OperationCatalog.Plane plane, String wireName, boolean idempotencyRequired,
        Class<R> requestType, Class<S> responseType) implements Operation {
        public TypedOperation {
            Objects.requireNonNull(plane, "plane");
            Objects.requireNonNull(wireName, "wireName");
            Objects.requireNonNull(requestType, "requestType");
            Objects.requireNonNull(responseType, "responseType");
            OperationCatalog.requireKnown(plane, wireName);
            if (OperationCatalog.requiresIdempotency(plane, wireName) != idempotencyRequired) {
                throw PlatformSdkException.invalidArgument();
            }
        }
    }

    public record AgentOperation(String wireName, boolean idempotencyRequired) implements Operation {
        public AgentOperation {
            Objects.requireNonNull(wireName, "wireName");
            if (!GeneratedContract.AGENT_OPERATIONS.contains(wireName)) {
                throw PlatformSdkException.invalidArgument();
            }
            if (idempotencyRequired != GeneratedContract.AGENT_IDEMPOTENT.contains(wireName)) {
                throw PlatformSdkException.invalidArgument();
            }
        }
        @Override public OperationCatalog.Plane plane() { return OperationCatalog.Plane.AGENT; }
    }

    public record HumanOperation(String wireName, OperationCatalog.Route route) implements Operation {
        public HumanOperation {
            Objects.requireNonNull(wireName, "wireName");
            Objects.requireNonNull(route, "route");
            if (!route.equals(GeneratedContract.HUMAN_ROUTES.get(wireName))) {
                throw PlatformSdkException.invalidArgument();
            }
        }
        @Override public OperationCatalog.Plane plane() { return OperationCatalog.Plane.HUMAN; }
        @Override public boolean idempotencyRequired() { return route.idempotency(); }
    }

    public sealed interface Request permits AgentRequest, HumanRequest {
        Operation operation();
        ObjectNode body();
    }

    public record AgentRequest(AgentOperation operation, ObjectNode body) implements Request {
        public AgentRequest {
            Objects.requireNonNull(operation, "operation");
            body = canonicalBody(body);
        }
    }

    public record HumanRequest(HumanOperation operation, ObjectNode body) implements Request {
        public HumanRequest {
            Objects.requireNonNull(operation, "operation");
            body = canonicalBody(body);
            if (operation.route().bodyless() && !body.isEmpty()) {
                throw PlatformSdkException.invalidArgument();
            }
        }
    }

    public sealed interface Response permits AgentResponse, HumanResponse {
        Operation operation();
        ObjectNode value();
    }

    public record AgentResponse(AgentOperation operation, ObjectNode value) implements Response {
        public AgentResponse {
            Objects.requireNonNull(operation, "operation");
            value = canonicalBody(value);
        }
    }

    public record HumanResponse(HumanOperation operation, ObjectNode value) implements Response {
        public HumanResponse {
            Objects.requireNonNull(operation, "operation");
            value = canonicalBody(value);
        }
    }

    public sealed interface Event permits AgentEvent, HumanEvent {
        String eventId();
        JsonNode value();
    }

    public record AgentEvent(String eventId, JsonNode value) implements Event {
        public AgentEvent {
            eventId = checkedText(eventId);
            value = checkedValue(value);
        }
    }

    public record HumanEvent(String eventId, JsonNode value) implements Event {
        public HumanEvent {
            eventId = checkedText(eventId);
            value = checkedValue(value);
        }
    }

    /** Immutable, percent-encoding-safe values for the path variables declared by a human operation. */
    public static final class PathParameters {
        private static final PathParameters NONE = new PathParameters(Map.of());
        private final Map<String, String> values;

        private PathParameters(Map<String, String> values) {
            var copy = new LinkedHashMap<String, String>();
            values.forEach((name, value) -> {
                if (name == null || name.isBlank() || value == null || value.isEmpty()
                        || name.indexOf('\0') >= 0 || value.indexOf('\0') >= 0) {
                    throw PlatformSdkException.invalidArgument();
                }
                copy.put(name, value);
            });
            this.values = Collections.unmodifiableMap(copy);
        }

        public static PathParameters none() { return NONE; }
        public static PathParameters of(Map<String, String> values) {
            return values == null || values.isEmpty() ? NONE : new PathParameters(values);
        }
        public static PathParameters of(String name, String value) {
            return new PathParameters(Map.of(name, value));
        }
        public String require(String name) {
            String value = values.get(name);
            if (value == null) throw PlatformSdkException.invalidArgument();
            return value;
        }
        Map<String, String> values() { return values; }
    }

    /** Converts a protocol integer only from its canonical decimal-string wire representation. */
    public static BigInteger protocolInteger(JsonNode value) {
        if (value == null || !value.isTextual()) throw PlatformSdkException.invalidArgument();
        String encoded = value.textValue();
        if (!encoded.equals("0") && (encoded.isEmpty() || encoded.charAt(0) == '0')) {
            throw PlatformSdkException.invalidArgument();
        }
        if (!encoded.chars().allMatch(character -> character >= '0' && character <= '9')) {
            throw PlatformSdkException.invalidArgument();
        }
        try {
            return ProtocolAmount.of(new BigInteger(encoded)).value();
        } catch (NumberFormatException error) {
            throw PlatformSdkException.invalidArgument();
        }
    }

    public static ObjectNode canonicalBody(ObjectNode value) {
        if (value == null) throw PlatformSdkException.invalidArgument();
        return value.deepCopy();
    }

    private static JsonNode checkedValue(JsonNode value) {
        if (value == null || value.isNull()) throw PlatformSdkException.invalidArgument();
        return value.deepCopy();
    }

    private static String checkedText(String value) {
        if (value == null || value.isEmpty() || value.indexOf('\0') >= 0) {
            throw PlatformSdkException.invalidArgument();
        }
        return value;
    }

    public static BigInteger protocolU64(BigInteger value) {
        if (value == null || value.signum() < 0 || value.bitLength() > 64) {
            throw PlatformSdkException.invalidArgument();
        }
        return value;
    }
}
