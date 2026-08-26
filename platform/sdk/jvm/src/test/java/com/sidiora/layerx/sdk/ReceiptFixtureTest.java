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
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final Path FIXTURE = Paths
        .get(System.getProperty("layerx.repo.root", "../../.."))
        .resolve("platform/sdk/conformance/fixtures/receipt-positive-v1.json");

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
