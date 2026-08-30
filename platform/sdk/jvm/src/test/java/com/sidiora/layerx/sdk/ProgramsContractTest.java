package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.math.BigInteger;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionStage;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

public final class ProgramsContractTest {
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final String HEX32 = "01".repeat(32);
    private static final String OTHER_HEX32 = "02".repeat(32);
    private static final String LAYERX_SECRET = "lxp_live_" + "a1".repeat(32);

    @Test
    void programsRoutesUseDirectPathsCanonicalGetBodiesAndLayerXKey() throws Exception {
        var credential = new HttpProductionTransport.LayerXKeyCredential("test_key",
            new SecretBytes(LAYERX_SECRET.getBytes(StandardCharsets.US_ASCII)));
        var transport = new HttpProductionTransport(HttpClient.newHttpClient(), JSON,
            URI.create("http://127.0.0.1:8080"), URI.create("http://127.0.0.1:9090/rpc"),
            Duration.ofSeconds(1), credential);
        ObjectNode selector = JSON.createObjectNode().put("program_id", HEX32)
            .put("requested_verification_level", "sequencer-signed");
        HttpRequest discover = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.discover", selector, SchemaTypes.PathParameters.of("program_id", HEX32), null));
        assertEquals("GET", discover.method());
        assertEquals("/v1/programs/registry/" + HEX32, discover.uri().getPath());
        assertTrue(discover.bodyPublisher().isPresent());
        assertTrue(discover.bodyPublisher().orElseThrow().contentLength() > 0);
        assertEquals("LayerX-Key test_key:" + LAYERX_SECRET,
            discover.headers().firstValue("Authorization").orElseThrow());
        assertFalse(discover.headers().firstValue("Idempotency-Key").isPresent());

        HttpRequest interfaceRequest = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.interface", selector, SchemaTypes.PathParameters.of("program_id", HEX32), null));
        assertEquals("GET", interfaceRequest.method());
        assertEquals("/v1/programs/registry/" + HEX32 + "/interface", interfaceRequest.uri().getPath());

        ObjectNode call = JSON.createObjectNode().put("program_id", HEX32).put("calldata", "")
            .put("signed_activity", "00");
        call.putObject("budget").put("fuel", "1").put("fee_limit", "0");
        call.putArray("capabilities");
        HttpRequest submission = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.call", call, SchemaTypes.PathParameters.none(), new IdempotencyKey(HEX32)));
        assertEquals("POST", submission.method());
        assertEquals("/v1/programs/call", submission.uri().getPath());
        assertEquals(HEX32, submission.headers().firstValue("Idempotency-Key").orElseThrow());

        HttpRequest simulation = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.simulate", call, SchemaTypes.PathParameters.none(), null));
        assertEquals("POST", simulation.method());
        assertEquals("/v1/programs/simulate", simulation.uri().getPath());

        ObjectNode receiptSelector = JSON.createObjectNode().put("idempotency_key", HEX32)
            .put("expected_activity_id", OTHER_HEX32)
            .put("requested_verification_level", "sequencer-signed");
        HttpRequest receipt = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.receipt", receiptSelector,
            SchemaTypes.PathParameters.of("idempotency_key", HEX32), null));
        assertEquals("GET", receipt.method());
        assertEquals("/v1/programs/receipts/by-idempotency/" + HEX32, receipt.uri().getPath());

        ObjectNode activitySelector = JSON.createObjectNode().put("activity_id", OTHER_HEX32)
            .put("requested_verification_level", "sequencer-signed");
        HttpRequest activity = transport.programRequest(new ProductionTransport.ProgramsCall(
            "program.activity", activitySelector,
            SchemaTypes.PathParameters.of("activity_id", OTHER_HEX32), null));
        assertEquals("GET", activity.method());
        assertEquals("/v1/programs/activities/" + OTHER_HEX32, activity.uri().getPath());

        selector.put("unexpected", true);
        assertThrows(PlatformSdkException.class, () -> transport.programRequest(
            new ProductionTransport.ProgramsCall("program.discover", selector,
                SchemaTypes.PathParameters.of("program_id", HEX32), null)));
        credential.close();
    }

    @Test
    void programsTransportRejectsBearerCredentialAndNonCanonicalCallKey() {
        var bearer = new HttpProductionTransport.BearerCredential(
            new SecretBytes("token".getBytes(StandardCharsets.US_ASCII)));
        var transport = new HttpProductionTransport(HttpClient.newHttpClient(), JSON,
            URI.create("http://localhost:8080"), URI.create("http://localhost:9090"),
            Duration.ofSeconds(1), bearer);
        ObjectNode call = JSON.createObjectNode().put("program_id", HEX32);
        var nonCanonical = new ProductionTransport.ProgramsCall("program.call", call,
            SchemaTypes.PathParameters.none(), new IdempotencyKey("ABC"));
        PlatformSdkException invalid = assertThrows(PlatformSdkException.class,
            () -> transport.programRequest(nonCanonical));
        assertEquals(PlatformSdkException.Code.IDEMPOTENCY_REQUIRED, invalid.code());

        var read = new ProductionTransport.ProgramsCall("program.activity",
            JSON.createObjectNode().put("activity_id", HEX32)
                .put("requested_verification_level", "sequencer-signed"),
            SchemaTypes.PathParameters.of("activity_id", HEX32), null);
        PlatformSdkException refused = assertThrows(PlatformSdkException.class,
            () -> transport.programRequest(read));
        assertEquals(PlatformSdkException.Code.CAPABILITY_REFUSAL, refused.code());
        bearer.close();
        assertThrows(PlatformSdkException.class, () -> new HttpProductionTransport(
            HttpClient.newBuilder().followRedirects(HttpClient.Redirect.ALWAYS).build(), JSON,
            URI.create("http://localhost:8080"), URI.create("http://localhost:9090"),
            Duration.ofSeconds(1), null));
    }

    @Test
    void programsAgentEnvelopeUsesExactOperationStatusMatrix() throws Exception {
        var credential = new HttpProductionTransport.LayerXKeyCredential("test",
            new SecretBytes(LAYERX_SECRET.getBytes(StandardCharsets.US_ASCII)));
        var transport = new HttpProductionTransport(HttpClient.newHttpClient(), JSON,
            URI.create("http://localhost:8080"), URI.create("http://localhost:9090"),
            Duration.ofSeconds(1), credential);
        JavaType object = JSON.constructType(ObjectNode.class);
        byte[] achieved = ("{\"request_id\":\"1\",\"value\":{},"
            + "\"verification_status\":{\"state\":\"Achieved\",\"level\":\"SequencerSigned\"}}")
            .getBytes(StandardCharsets.UTF_8);
        assertEquals(0, transport.<ObjectNode>decodePrograms("program.simulate", 200, achieved, object).size());

        byte[] downgraded = ("{\"request_id\":\"1\",\"value\":{},"
            + "\"verification_status\":{\"state\":\"Unverified\",\"level\":\"SequencerSigned\"}}")
            .getBytes(StandardCharsets.UTF_8);
        PlatformSdkException downgrade = assertThrows(PlatformSdkException.class,
            () -> transport.decodePrograms("program.simulate", 200, downgraded, object));
        assertEquals(PlatformSdkException.Code.DECODE_FAILURE, downgrade.code());

        byte[] extra = ("{\"request_id\":\"1\",\"value\":{},"
            + "\"verification_status\":{\"state\":\"Achieved\",\"level\":\"SequencerSigned\"},"
            + "\"extra\":true}").getBytes(StandardCharsets.UTF_8);
        assertThrows(PlatformSdkException.class,
            () -> transport.decodePrograms("program.simulate", 200, extra, object));

        byte[] serverVerifiedOnly = ("{\"request_id\":\"1\",\"value\":{},"
            + "\"verification_status\":{\"state\":\"Unverified\",\"level\":\"SequencerSigned\","
            + "\"reason\":\"server_side_receipt_verification_only\"}}")
            .getBytes(StandardCharsets.UTF_8);
        assertEquals(0, transport.<ObjectNode>decodePrograms(
            "program.discover", 200, serverVerifiedOnly, object).size());
        assertThrows(PlatformSdkException.class,
            () -> transport.decodePrograms("program.discover", 200, achieved, object));

        byte[] pending = ("{\"request_id\":\"1\",\"value\":{\"state\":\"unknown\"},"
            + "\"verification_status\":{\"state\":\"Unverified\",\"level\":\"SequencerSigned\","
            + "\"reason\":\"receipt_pending\"}}")
            .getBytes(StandardCharsets.UTF_8);
        assertEquals("unknown", transport.<ObjectNode>decodePrograms(
            "program.receipt", 200, pending, object).path("state").textValue());
        assertThrows(PlatformSdkException.class,
            () -> transport.decodePrograms("program.receipt", 200, achieved, object));

        byte[] serviceError = ("{\"class\":\"PolicyRefusal\",\"protocol_result_code\":null,"
            + "\"retriability\":\"Terminal\",\"request_id\":\"2\",\"reason\":\"policy_refusal\"}")
            .getBytes(StandardCharsets.UTF_8);
        PlatformSdkException error = assertThrows(PlatformSdkException.class,
            () -> transport.decodePrograms("program.discover", 403, serviceError, object));
        assertEquals(PlatformSdkException.Code.POLICY_REFUSAL, error.code());
        assertEquals("2", error.requestId());
        credential.close();
    }

    @Test
    void receiptSelectorBindsUnknownActivityAndIdempotency() {
        ObjectNode unknown = JSON.createObjectNode().put("state", "unknown")
            .put("activity_id", HEX32).put("idempotency_key", OTHER_HEX32);
        var transport = new CapturingTransport(unknown);
        byte[] pinnedKey = new byte[32];
        pinnedKey[0] = 3;
        var programs = new ProgramsClient(new ProductionClient(transport), pinnedKey);
        ProgramsClient.Submission result = programs.receipt(new IdempotencyKey(OTHER_HEX32),
            java.util.HexFormat.of().parseHex(HEX32)).toCompletableFuture().join();
        assertTrue(result.unknown());
        assertEquals(OTHER_HEX32, result.idempotencyKey());
        assertEquals("program.receipt", transport.call.operation());
        assertEquals(OTHER_HEX32, transport.call.pathParameters().require("idempotency_key"));

        unknown.put("activity_id", OTHER_HEX32);
        PlatformSdkException failure = assertInstanceOf(PlatformSdkException.class,
            assertThrows(java.util.concurrent.CompletionException.class,
                () -> programs.receipt(new IdempotencyKey(OTHER_HEX32),
                    java.util.HexFormat.of().parseHex(HEX32)).toCompletableFuture().join()).getCause());
        assertEquals(PlatformSdkException.Code.VERIFICATION_FAILURE, failure.code());
    }

    @Test
    void programCallBoundsAndCanonicalCapabilitiesFailClosed() {
        var budget = new ProgramsClient.Budget(BigInteger.ONE, BigInteger.ZERO);
        byte[] programId = new byte[32];
        programId[0] = 1;
        assertThrows(PlatformSdkException.class, () -> new ProgramsClient.Call(programId,
            new byte[ProgramsClient.MAX_CALLDATA_BYTES + 1], budget, List.of(), new byte[] {1}));
        assertThrows(PlatformSdkException.class, () -> new ProgramsClient.Call(programId,
            new byte[0], budget, List.of(ProgramsClient.Capability.TRANSFER,
                ProgramsClient.Capability.STORAGE_READ), new byte[] {1}));
        var transport = new CapturingTransport(JSON.createObjectNode());
        byte[] pinnedKey = new byte[32];
        pinnedKey[0] = 3;
        var programs = new ProgramsClient(new ProductionClient(transport), pinnedKey);
        ProgramsClient.Call call = new ProgramsClient.Call(programId, new byte[0], budget,
            List.of(ProgramsClient.Capability.STORAGE_READ), new byte[] {1});
        PlatformSdkException invalid = assertThrows(PlatformSdkException.class,
            () -> programs.submit(call, new IdempotencyKey("not-a-bytes32")));
        assertEquals(PlatformSdkException.Code.IDEMPOTENCY_REQUIRED, invalid.code());
    }

    @Test
    void submittedSignedActivityBindsCanonicalCallActivityAndIdempotency() throws Exception {
        byte[] programId = java.util.HexFormat.of().parseHex(HEX32);
        byte[] key = java.util.HexFormat.of().parseHex(OTHER_HEX32);
        byte[] calldata = new byte[] {9, 8};
        byte[] payload = programPayload(programId, calldata, key);
        byte[] signed = signedActivity(payload, key);
        byte[] activityId = sha256("LXP/v1/activity-id\0".getBytes(StandardCharsets.UTF_8), signed);
        ObjectNode unknown = JSON.createObjectNode().put("state", "unknown")
            .put("activity_id", java.util.HexFormat.of().formatHex(activityId))
            .put("idempotency_key", OTHER_HEX32)
            .put("retained_signed_activity", java.util.HexFormat.of().formatHex(signed));
        byte[] pinnedKey = new byte[32];
        pinnedKey[0] = 3;
        var programs = new ProgramsClient(new ProductionClient(new CapturingTransport(unknown)), pinnedKey);
        ProgramsClient.Call call = new ProgramsClient.Call(programId, calldata,
            new ProgramsClient.Budget(BigInteger.ONE, BigInteger.ZERO),
            List.of(ProgramsClient.Capability.STORAGE_READ), signed);
        ProgramsClient.Submission result = programs.submit(call, new IdempotencyKey(OTHER_HEX32))
            .toCompletableFuture().join();
        assertTrue(result.unknown());
        assertEquals(java.util.HexFormat.of().formatHex(activityId),
            java.util.HexFormat.of().formatHex(result.activityId()));
    }

    private static byte[] programPayload(byte[] programId, byte[] calldata, byte[] ignoredKey) {
        byte[] domain = "LayerX/programs/call/v1\0".getBytes(StandardCharsets.UTF_8);
        ByteBuffer out = ByteBuffer.allocate(domain.length + 32 + 8 + 16 + 2 + 1 + 4 + calldata.length);
        out.put(domain).put(programId).putLong(1).put(new byte[16]).putShort((short) 1).put((byte) 1)
            .putInt(calldata.length).put(calldata);
        return out.array();
    }

    private static byte[] signedActivity(byte[] payload, byte[] key) {
        byte[] payloadHash = sha256("LXP/v1/payload-hash\0".getBytes(StandardCharsets.UTF_8), payload);
        ByteBuffer out = ByteBuffer.allocate(157 + payload.length);
        out.putShort((short) 1).putShort((short) 0x1001).put((byte) 12);
        out.put((byte) 1).putShort((short) 1);
        out.put((byte) 2).putInt(1);
        out.put((byte) 3).putInt((9 << 16) | 3);
        out.put((byte) 4).putInt(1).put((byte) 'a');
        out.put((byte) 5).putInt(0);
        out.put((byte) 6).putLong(1);
        out.put((byte) 7).putLong(10).putLong(20);
        out.put((byte) 8).putInt(32).put(key);
        out.put((byte) 9).put(new byte[16]);
        out.put((byte) 10).putInt(32).put(payloadHash);
        out.put((byte) 11).putInt(payload.length).put(payload);
        out.put((byte) 12).putInt(1).put((byte) 0);
        return out.array();
    }

    private static byte[] sha256(byte[]... values) {
        try {
            var digest = java.security.MessageDigest.getInstance("SHA-256");
            for (byte[] value : values) digest.update(value);
            return digest.digest();
        } catch (java.security.GeneralSecurityException impossible) {
            throw new AssertionError(impossible);
        }
    }

    private static final class CapturingTransport implements ProductionTransport {
        private ObjectNode response;
        private ProgramsCall call;
        private CapturingTransport(ObjectNode response) { this.response = response; }

        @Override public <T> CompletionStage<T> call(Call call, JavaType responseType) {
            return CompletableFuture.failedFuture(new AssertionError("legacy transport used"));
        }

        @Override public <T> CompletionStage<T> callPrograms(ProgramsCall call, JavaType responseType) {
            this.call = call;
            @SuppressWarnings("unchecked") T value = (T) response;
            return CompletableFuture.completedFuture(value);
        }
    }
}
