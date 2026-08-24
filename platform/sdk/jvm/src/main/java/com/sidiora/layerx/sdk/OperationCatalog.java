package com.sidiora.layerx.sdk;

import java.util.List;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

/** Schema-generated operation inventory shared by the Java and Kotlin APIs. */
public final class OperationCatalog {
    private OperationCatalog() {}
    public enum Plane { AGENT, HUMAN }
    public record Route(String method, String path, List<String> pathParameters,
                        boolean idempotency, boolean bodyless) {
        public Route { pathParameters = List.copyOf(pathParameters); }
    }

    public static final Set<String> AGENT_OPERATIONS = GeneratedContract.AGENT_OPERATIONS;
    public static final Set<String> AGENT_IDEMPOTENT = GeneratedContract.AGENT_IDEMPOTENT;
    public static final Set<String> AGENT_ERROR_CLASSES = GeneratedContract.AGENT_ERROR_CLASSES;
    public static final Set<String> AGENT_RETRIABILITY = GeneratedContract.AGENT_RETRIABILITY;
    public static final Set<String> HUMAN_ERROR_CODES = GeneratedContract.HUMAN_ERROR_CODES;
    public static final Set<String> HUMAN_RETRIABILITY = GeneratedContract.HUMAN_RETRIABILITY;
    public static final Map<String, Route> HUMAN_ROUTES = GeneratedContract.HUMAN_ROUTES;
    public static final Map<String, SchemaTypes.AgentOperation> AGENT = agentOperations();
    public static final Map<String, SchemaTypes.HumanOperation> HUMAN = humanOperations();

    public static SchemaTypes.AgentOperation agent(String wireName) {
        SchemaTypes.AgentOperation operation = AGENT.get(wireName);
        if (operation == null) throw PlatformSdkException.invalidArgument();
        return operation;
    }

    public static SchemaTypes.HumanOperation human(String wireName) {
        SchemaTypes.HumanOperation operation = HUMAN.get(wireName);
        if (operation == null) throw PlatformSdkException.invalidArgument();
        return operation;
    }

    public static boolean requiresIdempotency(SchemaTypes.Operation operation) {
        return Objects.requireNonNull(operation, "operation").idempotencyRequired();
    }

    public static boolean requiresIdempotency(Plane plane, String operation) {
        Route human = HUMAN_ROUTES.get(operation);
        return plane == Plane.AGENT ? AGENT_IDEMPOTENT.contains(operation)
            : human != null && human.idempotency();
    }

    public static void requireKnown(Plane plane, String operation) {
        boolean known = plane == Plane.AGENT ? AGENT_OPERATIONS.contains(operation)
            : HUMAN_ROUTES.containsKey(operation);
        if (!known) throw PlatformSdkException.invalidArgument();
    }

    private static Map<String, SchemaTypes.AgentOperation> agentOperations() {
        var values = new LinkedHashMap<String, SchemaTypes.AgentOperation>();
        AGENT_OPERATIONS.forEach(name -> values.put(name,
            new SchemaTypes.AgentOperation(name, AGENT_IDEMPOTENT.contains(name))));
        return Map.copyOf(values);
    }

    private static Map<String, SchemaTypes.HumanOperation> humanOperations() {
        var values = new LinkedHashMap<String, SchemaTypes.HumanOperation>();
        HUMAN_ROUTES.forEach((name, route) -> values.put(name, new SchemaTypes.HumanOperation(name, route)));
        return Map.copyOf(values);
    }
}
