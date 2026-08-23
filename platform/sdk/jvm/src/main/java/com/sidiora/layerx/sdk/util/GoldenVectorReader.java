package com.sidiora.layerx.sdk.util;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;

/**
 * Utility for reading and validating LayerX golden vectors.
 * 
 * <p>Used by conformance tests and SDK validation.
 */
public final class GoldenVectorReader {
    private static final ObjectMapper JSON = new ObjectMapper();
    private final Path goldenRoot;

    public GoldenVectorReader(Path goldenRoot) {
        this.goldenRoot = goldenRoot;
    }

    public record Vector(String method, String path, Map<String, String> headers,
                        JsonNode body, int status, JsonNode responseBody) {}

    public Vector readOperation(String operation) throws IOException {
        Path prefix = goldenRoot.resolve(operation);
        JsonNode request = JSON.readTree(Files.readAllBytes(
            prefix.resolveSibling(prefix.getFileName() + ".request.json")));
        JsonNode response = JSON.readTree(Files.readAllBytes(
            prefix.resolveSibling(prefix.getFileName() + ".response.json")));

        Map<String, String> headers = new HashMap<>();
        if (request.has("headers")) {
            request.get("headers").fields().forEachRemaining(entry ->
                headers.put(entry.getKey(), entry.getValue().asText()));
        }

        return new Vector(
            request.get("method").asText(),
            request.get("path").asText(),
            headers,
            request.get("body"),
            response.get("status").asInt(),
            response.get("body"));
    }

    public boolean hasNumericMoney(JsonNode node, String fieldName) {
        if (node.isObject()) {
            var it = node.fields();
            while (it.hasNext()) {
                var entry = it.next();
                if ((entry.getKey().equals("amount") || entry.getKey().equals("amounts"))
                        && entry.getValue().isNumber()) {
                    return true;
                }
                if (hasNumericMoney(entry.getValue(), entry.getKey())) {
                    return true;
                }
            }
        } else if (node.isArray()) {
            for (JsonNode child : node) {
                if (hasNumericMoney(child, fieldName)) {
                    return true;
                }
            }
        }
        return false;
    }
}
