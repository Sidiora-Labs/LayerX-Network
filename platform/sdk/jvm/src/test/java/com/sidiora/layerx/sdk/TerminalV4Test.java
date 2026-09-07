package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.HexFormat;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

final class TerminalV4Test {
    private static byte[] bytes(JsonNode node, String name) { return HexFormat.of().parseHex(node.get(name).asText()); }

    @Test void signedSharedVectors() throws Exception {
        ObjectMapper json = new ObjectMapper();
        for (String name : new String[]{"executed-v4", "principal-v4", "mutated-leg-v4", "executed-v3"}) {
            JsonNode vector = json.readTree(Files.readString(Path.of(System.getProperty("layerx.repo.root", "../../.."), "platform/sdk/conformance/fixtures/receipt-programs-" + name + ".json")));
            JsonNode batch = vector.get("authorized_batch");
            var authority = new LocalVerifier.AuthorizedReceiptBatch(bytes(batch, "batch_id_hex"), bytes(batch, "asset_hex"), bytes(batch, "previous_state_root_hex"), bytes(batch, "resulting_state_root_hex"), bytes(batch, "sequencer_public_key_hex"));
            var verified = LocalVerifier.verifyReceipt(bytes(vector, "canonical_receipt_hex"), authority, 3);
            assertArrayEquals(bytes(vector, "receipt_digest_hex"), verified.receiptDigest());
            MessageDigest activity = MessageDigest.getInstance("SHA-256");
            activity.update("LXP/v1/activity-id\0".getBytes(StandardCharsets.UTF_8));
            assertArrayEquals(activity.digest(bytes(vector, "signed_activity_hex")), verified.receipt().activityId());
            byte[] terminal = bytes(vector, "terminal_payload_hex"), graph = bytes(vector, "call_graph_hex"), program = bytes(vector, "program_id_hex");
            var receipt = verified.receipt().programOutcome();
            assertNotNull(receipt);
            ObjectNode outcome = json.createObjectNode().put("kind", "completed").put("code", 0).put("response", "");
            if (name.equals("principal-v4")) {
                outcome.remove("response"); outcome.put("kind", "legacy_completed");
                outcome.putArray("values").addObject().put("type", "i32").put("value", 0);
            }
            if (name.equals("mutated-leg-v4")) {
                var error = assertThrows(IllegalArgumentException.class, () -> ProgramsClient.unwrapAppliedTerminal(terminal, receipt));
                assertEquals("applied transfer root", error.getMessage());
                assertThrows(PlatformSdkException.class, () -> ProgramsClient.verifyTerminal(terminal, graph, program, outcome, 3, receipt));
            } else {
                assertEquals(name.equals("executed-v3") ? "recorded_terminal_root_not_locally_reconstructable" : "reconstructed", ProgramsClient.verifyTerminal(terminal, graph, program, outcome, 3, receipt));
            }
            if (name.equals("executed-v4")) {
                for (int length = 0; length < terminal.length; length++) {
                    byte[] truncated = Arrays.copyOf(terminal, length);
                    assertThrows(PlatformSdkException.class, () -> ProgramsClient.verifyTerminal(truncated, graph, program, outcome, 3, receipt));
                }
                assertThrows(PlatformSdkException.class, () -> ProgramsClient.verifyTerminal(Arrays.copyOf(terminal, terminal.length + 1), graph, program, outcome, 3, receipt));
            }
        }
    }
}
