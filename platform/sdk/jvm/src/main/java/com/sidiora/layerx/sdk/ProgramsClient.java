package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.JsonNodeFactory;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.math.BigInteger;
import java.util.concurrent.CompletionStage;

public final class ProgramsClient {
    public static final int MAX_CALLDATA_BYTES = 1_048_576;
    public static final int MAX_CAPABILITIES = 5;
    public static final int PROGRAMS_RECEIPT_MODULE_ID = 9;
    public static final int CALL_OPERATION = 3;

    public enum Capability {
        STORAGE_READ("storage_read"), STORAGE_WRITE("storage_write"), TRANSFER("transfer"),
        EMIT_EVENT("emit_event"), COMPOSE("compose");
        private final String wire;
        Capability(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }
    public record Budget(BigInteger fuel, BigInteger feeLimit) {
        public Budget {
            if (fuel == null || fuel.signum() <= 0 || fuel.bitLength() > 64
                    || feeLimit == null || feeLimit.signum() < 0 || feeLimit.bitLength() > 128) invalid();
        }
    }
    public record Call(byte[] programId, byte[] calldata, Budget budget,
                       List<Capability> capabilities, byte[] signedActivity) {
        public Call {
            programId = exact(programId, 32);
            if (calldata == null || calldata.length > MAX_CALLDATA_BYTES || budget == null
                    || signedActivity == null || signedActivity.length == 0
                    || capabilities == null || capabilities.size() > MAX_CAPABILITIES) invalid();
            capabilities = List.copyOf(capabilities);
            if (capabilities.stream().anyMatch(Objects::isNull)) invalid();
            for (int index = 1; index < capabilities.size(); index++)
                if (capabilities.get(index - 1).ordinal() >= capabilities.get(index).ordinal()) invalid();
            calldata = calldata.clone(); signedActivity = signedActivity.clone();
        }
    }
    public record Discovery(ObjectNode value) { public Discovery { value = value.deepCopy(); } }
    public record Interface(ObjectNode value) { public Interface { value = value.deepCopy(); } }
    public record Simulation(ObjectNode value) { public Simulation { value = value.deepCopy(); } }
    public record Submission(ObjectNode value) {
        public Submission {
            value = value.deepCopy();
            String state = value.path("state").asText();
            if (!state.equals("refused") && !state.equals("unknown") && !state.equals("executed")) invalid();
        }
        public boolean unknown() { return value.path("state").asText().equals("unknown"); }
    }

    private final ProductionClient client;
    public ProgramsClient(ProductionClient client) { this.client = Objects.requireNonNull(client, "client"); }

    public CompletionStage<Discovery> discover(byte[] programId, String verificationLevel) {
        var id = hex(exact(programId, 32));
        var body = object().put("program_id", id).put("requested_verification_level", required(verificationLevel));
        return raw("program.discover", body, Map.of("program_id", id)).thenApply(Discovery::new);
    }
    public CompletionStage<Interface> interfaceAt(byte[] programId, String verificationLevel) {
        var id = hex(exact(programId, 32));
        var body = object().put("program_id", id).put("requested_verification_level", required(verificationLevel));
        return raw("program.interface", body, Map.of("program_id", id)).thenApply(Interface::new);
    }
    public CompletionStage<Simulation> simulate(Call call) { return raw("program.simulate", encode(call), Map.of()).thenApply(Simulation::new); }
    public CompletionStage<Submission> submit(Call call, IdempotencyKey key) {
        return client.agent("program.call", encode(call), ObjectNode.class,
            ProductionClient.Options.idempotent(Objects.requireNonNull(key, "key"))).thenApply(Submission::new);
    }
    public CompletionStage<Submission> receipt(IdempotencyKey key, byte[] expectedActivityId, String verificationLevel) {
        var activity = hex(exact(expectedActivityId, 32));
        var body = object().put("idempotency_key", key.value()).put("expected_activity_id", activity)
            .put("requested_verification_level", required(verificationLevel));
        return raw("program.receipt", body, Map.of("idempotency_key", key.value())).thenApply(Submission::new);
    }
    public CompletionStage<Submission> activity(byte[] activityId, String verificationLevel) {
        var id = hex(exact(activityId, 32));
        var body = object().put("activity_id", id).put("requested_verification_level", required(verificationLevel));
        return raw("program.activity", body, Map.of("activity_id", id)).thenApply(Submission::new);
    }

    public static LocalVerifier.ReceiptVerification verifyReceipt(byte[] canonicalReceipt,
            LocalVerifier.AuthorizedReceiptBatch authorized, byte[] expectedActivityId) {
        var verified = LocalVerifier.verifyReceiptOutcome(canonicalReceipt, authorized);
        var receipt = verified.receipt();
        if (receipt.protocolVersion() == 0 || receipt.moduleId() != PROGRAMS_RECEIPT_MODULE_ID
                || receipt.operation() != CALL_OPERATION || receipt.moduleVersion() < 1 || receipt.moduleVersion() > 3
                || !java.util.Arrays.equals(receipt.activityId(), exact(expectedActivityId, 32))) invalidVerification();
        return verified;
    }

    private CompletionStage<ObjectNode> raw(String operation, ObjectNode body, Map<String, String> path) {
        return client.agent(operation, body, ObjectNode.class, new ProductionClient.Options(null, path));
    }
    private static ObjectNode encode(Call call) {
        Objects.requireNonNull(call, "call");
        var value = object().put("program_id", hex(call.programId())).put("calldata", hex(call.calldata()))
            .put("signed_activity", hex(call.signedActivity()));
        value.set("budget", object().put("fuel", call.budget().fuel().toString()).put("fee_limit", call.budget().feeLimit().toString()));
        ArrayNode capabilities = value.putArray("capabilities");
        call.capabilities().forEach(item -> capabilities.add(item.wire()));
        return value;
    }
    private static ObjectNode object() { return JsonNodeFactory.instance.objectNode(); }
    private static String required(String value) { if (value == null || value.isEmpty() || value.length() > 64) invalid(); return value; }
    private static byte[] exact(byte[] value, int length) { if (value == null || value.length != length) invalid(); return value.clone(); }
    private static String hex(byte[] value) { return java.util.HexFormat.of().formatHex(value); }
    private static void invalid() { throw PlatformSdkException.invalidArgument(); }
    private static void invalidVerification() { throw new PlatformSdkException(PlatformSdkException.Code.VERIFICATION_FAILURE, PlatformSdkException.Retry.NEVER, null, null, null); }
}
