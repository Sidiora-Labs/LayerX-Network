package com.sidiora.layerx.android;

import java.util.LinkedHashMap;
import java.util.Map;

/** Typed refusal taxonomy for the Android binding, never carrying credential material. */
public final class MobileIntegrationException extends RuntimeException {
    public enum Code {
        INVALID_CONFIGURATION("invalid-configuration"), EMBEDDED_SECRET("embedded-secret"),
        INVALID_SESSION("invalid-session"), SESSION_EXPIRED("session-expired"),
        INVALID_EVENT("invalid-event"), EVENT_REPLAY("event-replay"),
        DELIVERY_STORE_FAILURE("delivery-store-failure"),
        VERIFICATION_FAILURE("verification-failure"), DECODE_FAILURE("decode-failure"),
        TRANSPORT_FAILURE("transport-failure"), UNAVAILABLE_CAPABILITY("unavailable-capability");
        private final String wire;
        Code(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }

    private static final Map<Code, String> MESSAGES = Map.ofEntries(
        Map.entry(Code.INVALID_CONFIGURATION, "The declared LayerX configuration is not usable."),
        Map.entry(Code.EMBEDDED_SECRET, "Secret material may not be embedded in a mobile artifact."),
        Map.entry(Code.INVALID_SESSION, "The session broker did not issue a usable ephemeral session."),
        Map.entry(Code.SESSION_EXPIRED, "The ephemeral session expired and must be re-brokered."),
        Map.entry(Code.INVALID_EVENT, "The delivered event failed signature or freshness verification."),
        Map.entry(Code.EVENT_REPLAY, "The delivery identifier was replayed with different payload bytes."),
        Map.entry(Code.DELIVERY_STORE_FAILURE, "The durable delivery ledger could not be read or committed."),
        Map.entry(Code.VERIFICATION_FAILURE, "Local receipt verification failed."),
        Map.entry(Code.DECODE_FAILURE, "The service response did not match the contract."),
        Map.entry(Code.TRANSPORT_FAILURE, "The request could not reach the service."),
        Map.entry(Code.UNAVAILABLE_CAPABILITY, "The requested operation is unavailable to a mobile binding."));

    private final Code code;

    public MobileIntegrationException(Code code) {
        super(MESSAGES.get(code));
        this.code = code;
    }

    public static MobileIntegrationException of(Code code) {
        return new MobileIntegrationException(code);
    }

    public Code code() { return code; }

    public Map<String, Object> safeDetails() {
        var value = new LinkedHashMap<String, Object>();
        value.put("code", code.wire());
        return Map.copyOf(value);
    }
}
