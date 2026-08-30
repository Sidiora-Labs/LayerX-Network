package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import org.junit.jupiter.api.Test;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import static org.junit.jupiter.api.Assertions.*;

public final class ReceiptFixtureTest {
    private static final String PROGRAM_OUTCOME_V3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000";

    @Test
    void programOutcomeV3VectorDecodes() {
        var outcome = LocalVerifier.decodeProgramReceiptOutcome(hexDecode(PROGRAM_OUTCOME_V3), 1);
        assertEquals(3, outcome.encodingVersion());
        assertEquals(1, outcome.abiVersion());
        assertEquals(java.math.BigInteger.valueOf(16), outcome.feeUnits());
        org.junit.jupiter.api.Assertions.assertArrayEquals(hexDecode("11".repeat(32)), outcome.callGraphRoot());
        org.junit.jupiter.api.Assertions.assertArrayEquals(hexDecode("22".repeat(32)), outcome.terminalPayloadRoot());
    }
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final Path FIXTURE = Paths
        .get(System.getProperty("layerx.repo.root", "../../.."))
        .resolve("platform/sdk/conformance/fixtures/receipt-positive-v1.json");
    private static final Path FIXTURE_ROOT = FIXTURE.getParent();

    @Test
    void testCoreFixtureReceiptVerifiesPositively() throws Exception {
        JsonNode fixture = JSON.readTree(Files.readString(FIXTURE));
        JsonNode expected = fixture.get("expected");
        byte[] canonical = hexDecode(fixture.get("canonical_receipt_hex").asText());
        LocalVerifier.ReceiptVerification verified =
            LocalVerifier.verifyReceipt(canonical, authorizedBatch(fixture));
        assertEquals(expected.get("level").asText(), verified.level().wire());
        assertArrayEquals(canonical, verified.canonicalBytes());
        assertArrayEquals(hexDecode(expected.get("receipt_digest_hex").asText()),
            verified.receiptDigest());
        LocalVerifier.ProtocolReceipt receipt = verified.receipt();
        assertEquals(expected.get("result_code").intValue(), receipt.resultCode());
        assertEquals(expected.get("protocol_version").intValue(), receipt.protocolVersion());
        assertEquals(expected.get("operation").intValue(), receipt.operation());
        assertEquals(expected.get("module_id").intValue(), receipt.moduleId());
        assertEquals(BigInteger.valueOf(expected.get("global_sequence").longValue()),
            receipt.globalSequence());
        assertEquals(BigInteger.valueOf(expected.get("timestamp_ms").longValue()),
            receipt.timestamp());
        assertEquals(new BigInteger(expected.get("amount").asText()), receipt.amount());
        assertEquals(new BigInteger(expected.get("fee_charged").asText()), receipt.feeCharged());
        assertEquals(new BigInteger(expected.get("from_balance_before").asText()),
            receipt.fromBalanceBefore());
        assertEquals(new BigInteger(expected.get("from_balance_after").asText()),
            receipt.fromBalanceAfter());
        assertEquals(new BigInteger(expected.get("to_balance_before").asText()),
            receipt.toBalanceBefore());
        assertEquals(new BigInteger(expected.get("to_balance_after").asText()),
            receipt.toBalanceAfter());
        assertArrayEquals(hexDecode(expected.get("activity_id_hex").asText()),
            receipt.activityId());
        assertArrayEquals(hexDecode(expected.get("from_hex").asText()), receipt.from());
        assertArrayEquals(hexDecode(expected.get("to_hex").asText()), receipt.to());
        JsonNode batch = fixture.get("authorized_batch");
        assertArrayEquals(hexDecode(batch.get("batch_id_hex").asText()), receipt.batchId());
        assertArrayEquals(hexDecode(batch.get("asset_hex").asText()), receipt.asset());
        assertArrayEquals(hexDecode(batch.get("previous_state_root_hex").asText()),
            receipt.previousStateRoot());
        assertArrayEquals(hexDecode(batch.get("resulting_state_root_hex").asText()),
            receipt.resultingStateRoot());
    }

    @Test
    void testCoreFixtureReceiptByteFlipFails() throws Exception {
        JsonNode fixture = JSON.readTree(Files.readString(FIXTURE));
        byte[] mutated = hexDecode(fixture.get("canonical_receipt_hex").asText());
        mutated[mutated.length - 1] ^= 0x01;
        assertThrows(PlatformSdkException.class,
            () -> LocalVerifier.verifyReceipt(mutated, authorizedBatch(fixture)));
    }

    @Test
    void programsReceiptPreservesOptionalOutcome() throws Exception {
        JsonNode fixture = JSON.readTree(Files.readString(
            FIXTURE_ROOT.resolve("receipt-programs-positive-v1.json")));
        LocalVerifier.ReceiptVerification verified = LocalVerifier.verifyReceipt(
            hexDecode(fixture.get("canonical_receipt_hex").asText()),
            authorizedBatch(fixture));
        LocalVerifier.ProgramReceiptOutcome outcome = verified.receipt().programOutcome();
        assertNotNull(outcome);
        assertEquals(3, outcome.encodingVersion());
        assertEquals(1, outcome.runtimeVersion());
        assertEquals(1, outcome.abiVersion());
        assertEquals(BigInteger.valueOf(16), outcome.feeUnits());
    }

    @Test
    void refusalVectorsExposeSharedTaxonomy() throws Exception {
        JsonNode fixture = JSON.readTree(Files.readString(
            FIXTURE_ROOT.resolve("receipt-refusals-v1.json")));
        for (JsonNode vector : fixture.get("vectors")) {
            PlatformSdkException failure = assertThrows(PlatformSdkException.class,
                () -> LocalVerifier.verifyReceipt(
                    hexDecode(vector.get("canonical_receipt_hex").asText()),
                    authorizedBatch(fixture)), vector.get("name").asText());
            assertNotNull(failure.receiptCheck());
            assertEquals(vector.get("expected_check").asText(),
                failure.receiptCheck().wire());
        }
    }

    private static LocalVerifier.AuthorizedReceiptBatch authorizedBatch(JsonNode fixture) {
        JsonNode batch = fixture.get("authorized_batch");
        return new LocalVerifier.AuthorizedReceiptBatch(
            hexDecode(batch.get("batch_id_hex").asText()),
            hexDecode(batch.get("asset_hex").asText()),
            hexDecode(batch.get("previous_state_root_hex").asText()),
            hexDecode(batch.get("resulting_state_root_hex").asText()),
            hexDecode(batch.get("sequencer_public_key_hex").asText()));
    }

    private static byte[] hexDecode(String hex) {
        assertEquals(0, hex.length() & 1, "hex must have even length");
        byte[] data = new byte[hex.length() / 2];
        for (int i = 0; i < hex.length(); i += 2) {
            data[i / 2] = (byte) ((Character.digit(hex.charAt(i), 16) << 4)
                + Character.digit(hex.charAt(i + 1), 16));
        }
        return data;
    }
}
