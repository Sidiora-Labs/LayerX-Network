package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.ByteArrayOutputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.HexFormat;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Flow;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

public final class NativeLifecycleTransportTest {
    @Test
    void signedCanonicalOctetsAndIdempotency() throws Exception {
        var json = new ObjectMapper(); var hex = HexFormat.of();
        try (var credential = new HttpProductionTransport.ProgramsBearerCredential(new SecretBytes("fixture-bearer".getBytes(StandardCharsets.US_ASCII)))) {
            var transport = new HttpProductionTransport(HttpClient.newHttpClient(), json, URI.create("http://127.0.0.1:8080"), URI.create("http://127.0.0.1:8080"), Duration.ofSeconds(1), credential);
            for (String name : new String[]{"deploy", "upgrade", "wind-down-route", "wind-down-deprecate", "wind-down-tombstone", "wind-down-exit"}) {
                var fixture = json.readTree(Files.readString(Path.of(System.getProperty("layerx.repo.root", "../../.."), "platform/sdk/conformance/fixtures/native-program-" + name + "-v3.json")));
                int ordinal = fixture.get("ordinal").intValue(); String operation = ordinal == 1 ? "program.deploy" : ordinal == 2 ? "program.upgrade" : "program.wind-down";
                String path = ordinal == 1 ? "/v1/programs/deploy" : ordinal == 2 ? "/v1/programs/upgrade" : "/v1/programs/wind-down";
                var body = json.createObjectNode().put("payload", fixture.get("payload_hex").asText()).put("signed_activity", fixture.get("signed_activity_hex").asText());
                var key = new IdempotencyKey(fixture.get("idempotency_key_hex").asText());
                var request = transport.programRequest(new ProductionTransport.ProgramsCall(operation, body, SchemaTypes.PathParameters.none(), key));
                assertEquals(path, request.uri().getPath()); assertEquals("POST", request.method());
                assertEquals("application/octet-stream", request.headers().firstValue("Content-Type").orElseThrow());
                assertEquals(key.value(), request.headers().firstValue("Idempotency-Key").orElseThrow());
                assertEquals("Bearer fixture-bearer", request.headers().firstValue("Authorization").orElseThrow());
                var result = new CompletableFuture<byte[]>(); var output = new ByteArrayOutputStream();
                request.bodyPublisher().orElseThrow().subscribe(new Flow.Subscriber<ByteBuffer>() {
                    @Override public void onSubscribe(Flow.Subscription subscription) { subscription.request(Long.MAX_VALUE); }
                    @Override public void onNext(ByteBuffer buffer) { byte[] bytes = new byte[buffer.remaining()]; buffer.get(bytes); output.writeBytes(bytes); }
                    @Override public void onError(Throwable error) { result.completeExceptionally(error); }
                    @Override public void onComplete() { result.complete(output.toByteArray()); }
                });
                assertArrayEquals(hex.parseHex(fixture.get("signed_activity_hex").asText()), result.join());
                assertThrows(PlatformSdkException.class, () -> transport.programRequest(new ProductionTransport.ProgramsCall(operation, body, SchemaTypes.PathParameters.none(), null)));
            }
        }
    }
}
