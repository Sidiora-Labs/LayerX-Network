package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import org.junit.jupiter.api.Test;
import java.io.IOException;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import static org.junit.jupiter.api.Assertions.*;

public final class GoldenVectorTest {
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final Path REPO_ROOT = Paths.get(System.getProperty("layerx.repo.root", "../../.."));

    @Test
    void testAgentApiVersion() throws Exception {
        Path goldenRoot = REPO_ROOT.resolve("agent/schema/agent-api/golden");
        byte[] request = hexDecode(readHex(goldenRoot.resolve("version-request.hex")));
        byte[] response = hexDecode(readHex(goldenRoot.resolve("version-response.hex")));
        assertNotNull(request);
        assertNotNull(response);
        assertTrue(request.length > 0);
        assertTrue(response.length > 0);
    }

    @Test
    void testCodecValidVectors() throws Exception {
        Path vectorPath = REPO_ROOT.resolve("tests/vectors/codec/valid.lxv");
        if (!Files.exists(vectorPath)) return;
        List<String> lines = Files.readAllLines(vectorPath);
        int verified = 0;
        for (String line : lines) {
            if (line.startsWith("#") || line.trim().isEmpty()) continue;
            String[] parts = line.split("\\|");
            if (parts.length < 3) continue;
            String kind = parts[0];
            String hex = parts[2];
            if (kind.equals("u64")) {
                byte[] bytes = hexDecode(hex);
                assertNotNull(bytes);
                assertEquals(8, bytes.length);
                verified++;
            }
        }
        assertTrue(verified > 0, "No valid codec vectors verified");
    }

    @Test
    void testProtocolAmountBounds() {
        assertThrows(PlatformSdkException.class, () -> 
            ProtocolAmount.of(BigInteger.valueOf(-1)));
        assertThrows(PlatformSdkException.class, () -> 
            ProtocolAmount.of(ProtocolAmount.MAX_VALUE.add(BigInteger.ONE)));
        
        ProtocolAmount zero = ProtocolAmount.of(BigInteger.ZERO);
        assertEquals("0", zero.toString());
        
        ProtocolAmount max = ProtocolAmount.of(ProtocolAmount.MAX_VALUE);
        assertTrue(max.toString().length() > 30);
    }

    @Test
    void testIdempotencyKeyValidation() {
        assertThrows(PlatformSdkException.class, () -> new IdempotencyKey(""));
        assertThrows(PlatformSdkException.class, () -> new IdempotencyKey(null));
        assertThrows(PlatformSdkException.class, () -> new IdempotencyKey("a\0b"));
        assertThrows(PlatformSdkException.class, () -> new IdempotencyKey("a".repeat(256)));
        
        IdempotencyKey valid = new IdempotencyKey("test-key-123");
        assertEquals("test-key-123", valid.toString());
    }

    @Test
    void testSecretBytesZeroization() {
        byte[] original = "secret-data".getBytes();
        SecretBytes secret = new SecretBytes(original);
        original[0] = (byte) 'X';
        
        secret.use(bytes -> {
            assertEquals('s', (char) bytes[0]);
            return null;
        });
        
        secret.close();
        assertTrue(secret.isDestroyed());
        assertThrows(PlatformSdkException.class, () -> secret.use(b -> null));
        assertEquals("[REDACTED]", secret.toString());
    }

    @Test
    void testResumableStreamCursorValidation() {
        assertThrows(PlatformSdkException.class, () -> 
            new ResumableStream.Cursor(""));
        assertThrows(PlatformSdkException.class, () -> 
            new ResumableStream.Cursor(null));
        assertThrows(PlatformSdkException.class, () -> 
            new ResumableStream.Cursor("a\0b"));
        assertThrows(PlatformSdkException.class, () -> 
            new ResumableStream.Cursor("x".repeat(513)));
        
        ResumableStream.Cursor valid = new ResumableStream.Cursor("cursor-abc-123");
        assertEquals("cursor-abc-123", valid.toString());
    }

    @Test
    void testMerkleProofDepthCalculation() {
        byte[] leaf = new byte[32];
        byte[] root = new byte[32];
        
        LocalVerifier.MerkleProof singleLeaf = new LocalVerifier.MerkleProof(0, 1, List.of());
        assertDoesNotThrow(() -> LocalVerifier.verifyMerkleInclusion(leaf, singleLeaf, root));
        
        LocalVerifier.MerkleProof twoLeaves = new LocalVerifier.MerkleProof(0, 2, List.of(new byte[32]));
        assertThrows(PlatformSdkException.class, () -> 
            LocalVerifier.verifyMerkleInclusion(leaf, twoLeaves, root));
    }

    @Test
    void testPlatformSdkMetadata() {
        var metadata = PlatformSdk.platform_sdk_jvm();
        assertNotNull(metadata);
        assertEquals("com.sidiora.layerx:layerx-sdk", metadata.get("name"));
        assertEquals("0.1.0", metadata.get("version"));
        assertEquals(1, metadata.get("contractMajor"));
        assertTrue((Integer) metadata.get("agentOperations") > 0);
        assertTrue((Integer) metadata.get("humanOperations") > 0);
    }

    @Test
    void testOperationCatalogIntegrity() {
        assertFalse(OperationCatalog.AGENT_OPERATIONS.isEmpty());
        assertFalse(OperationCatalog.HUMAN_ROUTES.isEmpty());
        assertFalse(OperationCatalog.HUMAN_ERROR_CODES.isEmpty());
        
        for (String operation : OperationCatalog.AGENT_OPERATIONS) {
            assertNotNull(operation);
            assertFalse(operation.isEmpty());
        }
        
        for (var entry : OperationCatalog.HUMAN_ROUTES.entrySet()) {
            assertNotNull(entry.getKey());
            assertNotNull(entry.getValue());
            assertNotNull(entry.getValue().method());
            assertNotNull(entry.getValue().path());
        }
    }

    private static String readHex(Path path) throws IOException {
        return Files.readString(path).trim();
    }

    private static byte[] hexDecode(String hex) {
        int len = hex.length();
        byte[] data = new byte[len / 2];
        for (int i = 0; i < len; i += 2) {
            data[i / 2] = (byte) ((Character.digit(hex.charAt(i), 16) << 4)
                + Character.digit(hex.charAt(i + 1), 16));
        }
        return data;
    }
}
