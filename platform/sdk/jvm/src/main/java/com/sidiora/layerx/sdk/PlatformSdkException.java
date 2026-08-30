package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.annotation.JsonValue;
import com.sidiora.layerx.sdk.verify.GeneratedReceiptContract.ReceiptCheck;
import java.util.Map;

public final class PlatformSdkException extends RuntimeException {
    public enum Code {
        INVALID_ARGUMENT("invalid-argument"), IDEMPOTENCY_REQUIRED("idempotency-required"),
        TRANSPORT_FAILURE("transport-failure"), DEADLINE("deadline"),
        PROTOCOL_INCOMPATIBILITY("protocol-incompatibility"), UNAVAILABLE_CAPABILITY("unavailable-capability"),
        CORE_REJECTION("core-rejection"), VERIFICATION_FAILURE("verification-failure"),
        POLICY_REFUSAL("policy-refusal"), CAPABILITY_REFUSAL("capability-refusal"),
        BUDGET_REFUSAL("budget-refusal"), RATE_LIMIT("rate-limit"),
        IDEMPOTENCY_CONFLICT("idempotency-conflict"), DECODE_FAILURE("decode-failure"),
        UNKNOWN_OUTCOME("unknown-outcome"), INTERNAL_FAULT("internal-fault");
        private final String wire;
        Code(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }
    public enum Retry { NEVER("never"), SAFE("safe"), AFTER("after"), UNKNOWN_OUTCOME("unknown-outcome");
        private final String wire;
        Retry(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }

    private static final Map<Code, String> MESSAGES = Map.ofEntries(
        Map.entry(Code.INVALID_ARGUMENT, "The SDK rejected an invalid argument."),
        Map.entry(Code.IDEMPOTENCY_REQUIRED, "This operation requires an idempotency key."),
        Map.entry(Code.TRANSPORT_FAILURE, "The request could not reach the service."),
        Map.entry(Code.DEADLINE, "The request deadline elapsed."),
        Map.entry(Code.PROTOCOL_INCOMPATIBILITY, "The service protocol is not compatible with this SDK."),
        Map.entry(Code.UNAVAILABLE_CAPABILITY, "The requested operation is unavailable."),
        Map.entry(Code.CORE_REJECTION, "The protocol refused the request."),
        Map.entry(Code.VERIFICATION_FAILURE, "Local verification failed."),
        Map.entry(Code.POLICY_REFUSAL, "Policy refused the request."),
        Map.entry(Code.CAPABILITY_REFUSAL, "The caller does not have the required authority."),
        Map.entry(Code.BUDGET_REFUSAL, "The configured budget refused the request."),
        Map.entry(Code.RATE_LIMIT, "The request rate limit was reached."),
        Map.entry(Code.IDEMPOTENCY_CONFLICT, "The idempotency key belongs to a different request."),
        Map.entry(Code.DECODE_FAILURE, "The service response did not match the contract."),
        Map.entry(Code.UNKNOWN_OUTCOME, "The request outcome is unknown and must be resolved before retrying."),
        Map.entry(Code.INTERNAL_FAULT, "The service could not complete the request."));

    private final Code code;
    private final Retry retry;
    private final String requestId;
    private final Integer protocolResultCode;
    private final Long retryAfterMs;
    private final SchemaErrors.AgentClass agentClass;
    private final SchemaErrors.AgentRetriability agentRetriability;
    private final SchemaErrors.HumanCode humanCode;
    private final SchemaErrors.HumanRetriability humanRetriability;
    private final ReceiptCheck receiptCheck;

    public PlatformSdkException(Code code, Retry retry, String requestId, Integer protocolResultCode, Long retryAfterMs) {
        this(code, retry, requestId, protocolResultCode, retryAfterMs, null, null, null, null, null);
    }

    private PlatformSdkException(Code code, Retry retry, String requestId, Integer protocolResultCode,
                                 Long retryAfterMs, SchemaErrors.AgentClass agentClass,
                                 SchemaErrors.AgentRetriability agentRetriability,
                                 SchemaErrors.HumanCode humanCode,
                                 SchemaErrors.HumanRetriability humanRetriability,
                                 ReceiptCheck receiptCheck) {
        super(MESSAGES.get(code));
        this.code = code;
        this.retry = retry;
        this.requestId = requestId;
        this.protocolResultCode = protocolResultCode;
        this.retryAfterMs = retryAfterMs;
        this.agentClass = agentClass;
        this.agentRetriability = agentRetriability;
        this.humanCode = humanCode;
        this.humanRetriability = humanRetriability;
        this.receiptCheck = receiptCheck;
    }

    public static PlatformSdkException agent(Code code, Retry retry, String requestId,
                                             Integer protocolResultCode, Long retryAfterMs,
                                             SchemaErrors.AgentClass agentClass,
                                             SchemaErrors.AgentRetriability retriability) {
        if (agentClass == null || retriability == null) throw invalidArgument();
        return new PlatformSdkException(code, retry, requestId, protocolResultCode, retryAfterMs,
            agentClass, retriability, null, null, null);
    }

    public static PlatformSdkException human(Code code, Retry retry, String requestId,
                                             Integer protocolResultCode, Long retryAfterMs,
                                             SchemaErrors.HumanCode humanCode,
                                             SchemaErrors.HumanRetriability retriability) {
        if (humanCode == null || retriability == null) throw invalidArgument();
        return new PlatformSdkException(code, retry, requestId, protocolResultCode, retryAfterMs,
            null, null, humanCode, retriability, null);
    }

    public static PlatformSdkException invalidArgument() {
        return new PlatformSdkException(Code.INVALID_ARGUMENT, Retry.NEVER, null, null, null);
    }
    public static PlatformSdkException verificationFailure() {
        return new PlatformSdkException(Code.VERIFICATION_FAILURE, Retry.NEVER, null, null, null);
    }
    public static PlatformSdkException receiptVerification(ReceiptCheck check) {
        if (check == null) throw invalidArgument();
        return new PlatformSdkException(Code.VERIFICATION_FAILURE, Retry.NEVER, null, null, null,
            null, null, null, null, check);
    }
    public Code code() { return code; }
    public Retry retry() { return retry; }
    public String requestId() { return requestId; }
    public Integer protocolResultCode() { return protocolResultCode; }
    public Long retryAfterMs() { return retryAfterMs; }
    public SchemaErrors.AgentClass agentClass() { return agentClass; }
    public SchemaErrors.AgentRetriability agentRetriability() { return agentRetriability; }
    public SchemaErrors.HumanCode humanCode() { return humanCode; }
    public SchemaErrors.HumanRetriability humanRetriability() { return humanRetriability; }
    public ReceiptCheck receiptCheck() { return receiptCheck; }
    @JsonValue public Map<String, Object> safeDetails() {
        var value = new java.util.LinkedHashMap<String, Object>();
        value.put("code", code.wire()); value.put("retry", retry.wire());
        if (requestId != null) value.put("requestId", requestId);
        if (protocolResultCode != null) value.put("protocolResultCode", protocolResultCode);
        if (retryAfterMs != null) value.put("retryAfterMs", retryAfterMs);
        if (agentClass != null) value.put("agentClass", agentClass.wire());
        if (agentRetriability != null) value.put("agentRetriability", agentRetriability.wire());
        if (humanCode != null) value.put("humanCode", humanCode.wire());
        if (humanRetriability != null) value.put("humanRetriability", humanRetriability.wire());
        if (receiptCheck != null) value.put("receiptCheck", receiptCheck.wire());
        return Map.copyOf(value);
    }
}
