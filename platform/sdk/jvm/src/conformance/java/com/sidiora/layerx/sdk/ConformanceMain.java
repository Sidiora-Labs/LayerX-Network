package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.io.IOException;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;

/** Schema-golden conformance gate, compiled only by the conformance Maven profile. */
public final class ConformanceMain {
    private static final ObjectMapper JSON = new ObjectMapper();
    private ConformanceMain() {}

    public static void main(String[] arguments) throws Exception {
        Path root = arguments.length == 0 ? Path.of("../../..") : Path.of(arguments[0]);
        Path golden = root.resolve("human/schema/human-api/golden");
        for (Map.Entry<String, OperationCatalog.Route> operation : OperationCatalog.HUMAN_ROUTES.entrySet()) {
            verifyTriplet(golden, operation.getKey(), operation.getValue());
        }
        int agentGoldenValues = verifyAgentGoldens(root.resolve("agent/schema/agent-api/golden"));
        verifySchemaParity();
        verifyProtocolIntegers();
        verifyStreamAndSecrets();
        invokeLocalVerification();
        Set<String> sdkCodes = new HashSet<>();
        for (PlatformSdkException.Code code : PlatformSdkException.Code.values()) {
            if (!sdkCodes.add(code.wire())) throw new IllegalStateException("duplicate SDK error code");
        }
        System.out.printf("agent_operations=%d human_operations=%d human_golden_triplets=%d agent_goldens=%d sdk_error_codes=%d%n",
            OperationCatalog.AGENT_OPERATIONS.size(), OperationCatalog.HUMAN_ROUTES.size(),
            OperationCatalog.HUMAN_ROUTES.size(), agentGoldenValues, sdkCodes.size());
    }

    private static void verifyTriplet(Path golden, String operation, OperationCatalog.Route route) throws IOException {
        Path prefix = golden.resolve(operation);
        JsonNode request = read(prefix.resolveSibling(prefix.getFileName() + ".request.json"));
        if (!route.method().equals(text(request, "method")) || !pathMatches(route.path(), text(request, "path"))) {
            throw new IllegalStateException(operation + " request method/path diverges from schema");
        }
        JsonNode headers = request.path("headers");
        if (route.idempotency() && (!headers.isObject() || headers.path("Idempotency-Key").asText().isEmpty())) {
            throw new IllegalStateException(operation + " request omits Idempotency-Key");
        }
        rejectNumericMoney(request.path("body"), "body");
        verifySuccess(operation, read(prefix.resolveSibling(prefix.getFileName() + ".response.json")));
        verifyFailure(operation, read(prefix.resolveSibling(prefix.getFileName() + ".failure.json")));
    }

    private static void verifySuccess(String operation, JsonNode vector) {
        int status = vector.path("status").asInt(-1);
        JsonNode body = vector.path("body");
        if (status < 200 || status >= 300 || !body.path("ok").asBoolean(false)
                || body.path("trace").asText().isEmpty() || !body.has("result") || body.has("error")) {
            throw new IllegalStateException(operation + " success vector is not a typed success envelope");
        }
        rejectNumericMoney(body.path("result"), "result");
    }

    private static void verifyFailure(String operation, JsonNode vector) {
        int status = vector.path("status").asInt(-1);
        JsonNode body = vector.path("body"), error = body.path("error");
        String retry = error.path("retry").asText();
        if ((status >= 200 && status < 300) || body.path("ok").asBoolean(true)
                || body.path("trace").asText().isEmpty()
                || !OperationCatalog.HUMAN_ERROR_CODES.contains(error.path("code").asText())
                || !OperationCatalog.HUMAN_RETRIABILITY.contains(retry)) {
            throw new IllegalStateException(operation + " failure vector is not a typed failure envelope");
        }
        SchemaErrors.HumanCode.fromWire(error.path("code").asText());
        SchemaErrors.HumanRetriability.fromWire(retry);
        if (retry.equals("retriable-after") && !error.path("retry_after_ms").canConvertToLong()) {
            throw new IllegalStateException(operation + " retriable-after failure omits retry_after_ms");
        }
    }

    private static int verifyAgentGoldens(Path root) throws IOException {
        int verified = 0;
        List<Path> schemaFiles;
        try (var paths = Files.list(root)) {
            schemaFiles = paths.filter(file -> file.toString().endsWith(".kvx")).sorted().toList();
        }
        for (Path path : schemaFiles) {
            String section = null;
            for (String line : Files.readAllLines(path)) {
                String trimmed = line.trim();
                if (trimmed.startsWith("[golden.") && trimmed.endsWith("]")) section = trimmed;
                if (section != null && trimmed.startsWith("encoded_hex = \"") && trimmed.endsWith("\"")) {
                    String encoded = trimmed.substring(15, trimmed.length() - 1);
                    byte[] bytes = HexFormat.of().parseHex(encoded);
                    if (bytes.length == 0) throw new IllegalStateException(path + " " + section + " is empty");
                    if (bytes[0] != '{') {
                        boolean versionVector = section.contains(".version")
                            && (bytes.length == 19 || bytes.length == 23)
                            && bytes[0] == 0 && bytes[1] == 1;
                        if (!versionVector) throw new IllegalStateException(path + " " + section + " is not canonical Agent wire data");
                        verified++;
                        continue;
                    }
                    JsonNode value = JSON.readTree(bytes);
                    if (value == null || !value.isObject() || !MessageDigest.isEqual(bytes, JSON.writeValueAsBytes(value))) {
                        throw new IllegalStateException(path + " " + section + " is not canonical JSON");
                    }
                    rejectNumericMoney(value, section);
                    verified++;
                }
            }
        }
        if (verified == 0) throw new IllegalStateException("agent golden corpus is empty");
        return verified;
    }

    private static void verifySchemaParity() {
        if (!GeneratedSchema.AGENT.keySet().equals(OperationCatalog.AGENT_OPERATIONS)
                || !GeneratedSchema.HUMAN.keySet().equals(OperationCatalog.HUMAN_ROUTES.keySet())) {
            throw new IllegalStateException("generated typed operation catalogue diverges from live schemas");
        }
        if (!OperationCatalog.AGENT_ERROR_CLASSES.equals(wires(SchemaErrors.AgentClass.values()))
                || !OperationCatalog.AGENT_RETRIABILITY.equals(wires(SchemaErrors.AgentRetriability.values()))
                || !OperationCatalog.HUMAN_ERROR_CODES.equals(wires(SchemaErrors.HumanCode.values()))
                || !OperationCatalog.HUMAN_RETRIABILITY.equals(wires(SchemaErrors.HumanRetriability.values()))) {
            throw new IllegalStateException("JVM error taxonomy diverges from generated schema metadata");
        }
        OperationCatalog.AGENT_OPERATIONS.forEach(name -> {
            if (!OperationCatalog.agent(name).wireName().equals(name)) throw new IllegalStateException("agent operation drift");
        });
        OperationCatalog.HUMAN_ROUTES.forEach((name, route) -> {
            if (!OperationCatalog.human(name).route().equals(route)) throw new IllegalStateException("human operation drift");
        });
    }

    private static Set<String> wires(SchemaErrors.WireValue[] values) {
        Set<String> wires = new HashSet<>();
        for (SchemaErrors.WireValue value : values) wires.add(value.wire());
        return Set.copyOf(wires);
    }

    private static void verifyProtocolIntegers() throws IOException {
        if (!SchemaTypes.protocolInteger(JSON.readTree("\"340282366920938463463374607431768211455\"")).equals(
                ProtocolAmount.MAX_VALUE)) throw new IllegalStateException("u128 maximum changed");
        for (String rejected : List.of("1.0", "1", "\"01\"", "\"-1\"", "\"340282366920938463463374607431768211456\"")) {
            try {
                SchemaTypes.protocolInteger(JSON.readTree(rejected));
                throw new IllegalStateException("non-canonical protocol integer accepted: " + rejected);
            } catch (PlatformSdkException expected) {
                if (expected.code() != PlatformSdkException.Code.INVALID_ARGUMENT) throw expected;
            }
        }
    }

    private static void verifyStreamAndSecrets() {
        var initial = new ResumableStream.Cursor("cursor-0");
        var next = new ResumableStream.Cursor("cursor-1");
        var stream = new ResumableStream<String>(initial);
        var event = new ResumableStream.Event<>("event-1", initial, next, "value");
        if (!stream.accept(new ResumableStream.Page<>(initial, List.of(event), next)).equals(List.of(event))
                || !stream.cursor().equals(next)) throw new IllegalStateException("cursor did not advance atomically");
        try {
            stream.accept(new ResumableStream.Page<>(next, List.of(event), next));
            throw new IllegalStateException("duplicate stream event accepted");
        } catch (PlatformSdkException expected) {
            if (expected.code() != PlatformSdkException.Code.DECODE_FAILURE) throw expected;
        }
        byte[] input = "conformance-secret".getBytes(java.nio.charset.StandardCharsets.UTF_8);
        SecretBytes secret = new SecretBytes(input);
        input[0] = 0;
        secret.close();
        if (!secret.isDestroyed() || !secret.toString().equals("[REDACTED]")) {
            throw new IllegalStateException("secret lifecycle is not fail-closed");
        }
    }

    private static void invokeLocalVerification() throws Exception {
        byte[] leaf = "layerx-jvm-conformance".getBytes(java.nio.charset.StandardCharsets.UTF_8);
        byte[] root = MessageDigest.getInstance("SHA-256").digest(concat(
            "LXP/v1/merkle-leaf\0".getBytes(java.nio.charset.StandardCharsets.UTF_8), leaf));
        LocalVerifier.verifyMerkleInclusion(leaf, new LocalVerifier.MerkleProof(0, 1, List.of()), root);
        expectVerificationFailure(() -> LocalVerifier.verifyReceipt(new byte[] {0},
            new LocalVerifier.AuthorizedReceiptBatch(new byte[32], new byte[32], new byte[32],
                new byte[32], new byte[32])));
        expectVerificationFailure(() -> LocalVerifier.verifyBatchInclusion(LocalVerifier.InclusionKind.RECEIPT,
            leaf, new LocalVerifier.MerkleProof(0, 1, List.of()), new byte[0], new byte[64],
            new LocalVerifier.SequencerAuthorization(new byte[32], new byte[32], BigInteger.ZERO, BigInteger.ZERO)));
        expectVerificationFailure(() -> LocalVerifier.verifyCheckpoint(new LocalVerifier.CheckpointVerificationInput(
            new LocalVerifier.CheckpointCertificate(new byte[0], new byte[0], List.of(), 1, null),
            List.of(), new byte[32], null, true)));
    }

    private static void expectVerificationFailure(Runnable action) {
        try {
            action.run();
            throw new IllegalStateException("invalid verification input accepted");
        } catch (PlatformSdkException expected) {
            if (expected.code() != PlatformSdkException.Code.VERIFICATION_FAILURE) throw expected;
        }
    }

    private static byte[] concat(byte[] left, byte[] right) {
        byte[] value = java.util.Arrays.copyOf(left, left.length + right.length);
        System.arraycopy(right, 0, value, left.length, right.length);
        return value;
    }

    private static void rejectNumericMoney(JsonNode node, String name) {
        if (node.isObject()) node.fields().forEachRemaining(field -> rejectNumericMoney(field.getValue(), field.getKey()));
        else if (node.isArray()) node.forEach(child -> rejectNumericMoney(child, name));
        else if ((name.equals("amount") || name.equals("amounts") || name.endsWith("_amount")
                || name.endsWith("_balance") || name.endsWith("_sequence")) && node.isNumber()) {
            throw new IllegalStateException(name + " is encoded as a JSON number");
        }
    }

    private static boolean pathMatches(String template, String actual) {
        StringBuilder expression = new StringBuilder("^");
        int position = 0;
        var parameter = Pattern.compile("\\{[^}]+}").matcher(template);
        while (parameter.find()) {
            expression.append(Pattern.quote(template.substring(position, parameter.start()))).append("[^/]+");
            position = parameter.end();
        }
        expression.append(Pattern.quote(template.substring(position))).append('$');
        return actual.matches(expression.toString());
    }
    private static String text(JsonNode node, String field) {
        JsonNode value = node.path(field);
        if (!value.isTextual()) throw new IllegalStateException(field + " is missing");
        return value.textValue();
    }
    private static JsonNode read(Path path) throws IOException { return JSON.readTree(Files.readAllBytes(path)); }
}
