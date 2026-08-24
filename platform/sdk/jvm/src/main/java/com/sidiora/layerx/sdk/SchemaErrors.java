package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.annotation.JsonValue;
/** Stable schema error codes and retriability classifications for both public API planes. */
public final class SchemaErrors {
    static {
        requireParity(AgentClass.values(), GeneratedContract.AGENT_ERROR_CLASSES);
        requireParity(AgentRetriability.values(), GeneratedContract.AGENT_RETRIABILITY);
        requireParity(HumanCode.values(), GeneratedContract.HUMAN_ERROR_CODES);
        requireParity(HumanRetriability.values(), GeneratedContract.HUMAN_RETRIABILITY);
    }
    private SchemaErrors() {}

    public interface WireValue { @JsonValue String wire(); }

    public enum AgentClass implements WireValue {
        TRANSPORT_FAILURE("TransportFailure"), DEADLINE("Deadline"),
        PROTOCOL_INCOMPATIBILITY("ProtocolIncompatibility"),
        UNAVAILABLE_CAPABILITY("UnavailableCapability"), CORE_REJECTION("CoreRejection"),
        VERIFICATION_FAILURE("VerificationFailure"), POLICY_REFUSAL("PolicyRefusal"),
        CAPABILITY_REFUSAL("CapabilityRefusal"), BUDGET_REFUSAL("BudgetRefusal"),
        RATE_LIMIT("RateLimit"), IDEMPOTENCY_CONFLICT("IdempotencyConflict"),
        INTERNAL_FAULT("InternalFault");
        private final String wire;
        AgentClass(String wire) { this.wire = wire; }
        @Override public String wire() { return wire; }
        public static AgentClass fromWire(String wire) { return parse(values(), wire); }
    }

    public enum AgentRetriability implements WireValue {
        TERMINAL("Terminal"), RETRIABLE("Retriable");
        private final String wire;
        AgentRetriability(String wire) { this.wire = wire; }
        @Override public String wire() { return wire; }
        public static AgentRetriability fromWire(String wire) { return parse(values(), wire); }
    }

    public enum HumanCode implements WireValue {
        UNAUTHENTICATED("unauthenticated"), SESSION_EXPIRED("session-expired"),
        STEP_UP_REQUIRED("step-up-required"), FORBIDDEN("forbidden"), NOT_FOUND("not-found"),
        INVALID_REQUEST("invalid-request"), CONFLICT("conflict"), RATE_LIMITED("rate-limited"),
        CURSOR_EXPIRED("cursor-expired"), UNAVAILABLE("unavailable"),
        UPSTREAM_DEGRADED("upstream-degraded"), CHALLENGE_EXPIRED("challenge-expired"),
        REFUSED_BY_POLICY("refused-by-policy"), REFUSED_BY_BUDGET("refused-by-budget"),
        REFUSED_BY_CAPABILITY("refused-by-capability"), REFUSED_BY_PROTOCOL("refused-by-protocol"),
        REFUSED_BY_LIMIT("refused-by-limit"), QUOTE_EXPIRED("quote-expired"),
        WALLET_NOT_BOUND("wallet-not-bound"), EXIT_UNAVAILABLE("exit-unavailable"),
        ALREADY_DECIDED("already-decided"), HOLD_EXPIRED("hold-expired"),
        HOLD_DEFECTIVE("hold-defective"), ARCHIVE_NEEDS_DISPOSITION("archive-needs-disposition"),
        CONFIRMATION_MISMATCH("confirmation-mismatch"), NOT_SUPPRESSIBLE("not-suppressible"),
        SUPPORT_UNAVAILABLE("support-unavailable"),
        SUPPORT_CONVERSATION_UNKNOWN("support-conversation-unknown"),
        SUPPORT_MESSAGE_UNKNOWN("support-message-unknown");
        private final String wire;
        HumanCode(String wire) { this.wire = wire; }
        @Override public String wire() { return wire; }
        public static HumanCode fromWire(String wire) { return parse(values(), wire); }
    }

    public enum HumanRetriability implements WireValue {
        RETRIABLE("retriable"), RETRIABLE_AFTER("retriable-after"),
        STRUCTURAL("structural"), FINAL("final");
        private final String wire;
        HumanRetriability(String wire) { this.wire = wire; }
        @Override public String wire() { return wire; }
        public static HumanRetriability fromWire(String wire) { return parse(values(), wire); }
    }

    private static <T extends Enum<T> & WireValue> T parse(T[] values, String wire) {
        if (wire != null) {
            for (T value : values) if (value.wire().equals(wire)) return value;
        }
        throw new IllegalArgumentException("unknown schema value");
    }

    private static <T extends Enum<T> & WireValue> void requireParity(T[] values,
                                                                      java.util.Set<String> generated) {
        java.util.Set<String> declared = java.util.Arrays.stream(values)
            .map(WireValue::wire).collect(java.util.stream.Collectors.toUnmodifiableSet());
        if (!declared.equals(generated)) throw new ExceptionInInitializerError("generated schema enum drift");
    }
}
