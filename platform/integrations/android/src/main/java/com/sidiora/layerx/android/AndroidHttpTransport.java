package com.sidiora.layerx.android;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.OperationCatalog;
import com.sidiora.layerx.sdk.PlatformSdkException;
import com.sidiora.layerx.sdk.ProductionTransport;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.ProtocolException;
import java.net.URI;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.Executor;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Human-plane HTTP transport built on the connection stack Android actually ships. */
public final class AndroidHttpTransport implements ProductionTransport, AutoCloseable {
    private static final int MAXIMUM_RESPONSE_BYTES = 8 * 1024 * 1024;
    private static final int MAXIMUM_REQUEST_BYTES = 1024 * 1024;

    private final URI humanBaseUri;
    private final SessionTokenProvider sessions;
    private final ObjectMapper mapper;
    private final int timeoutMs;
    private final Executor executor;
    private final ExecutorService owned;

    private AndroidHttpTransport(URI humanBaseUri, SessionTokenProvider sessions, ObjectMapper mapper,
                                 int timeoutMs, Executor executor, ExecutorService owned) {
        this.humanBaseUri = humanBaseUri;
        this.sessions = sessions;
        this.mapper = mapper;
        this.timeoutMs = timeoutMs;
        this.executor = executor;
        this.owned = owned;
    }

    public AndroidHttpTransport(URI humanBaseUri, SessionTokenProvider sessions, ObjectMapper mapper,
                                int timeoutMs, Executor executor) {
        this(PublishableConfiguration.endpoint(Objects.requireNonNull(humanBaseUri, "humanBaseUri").toString()),
            Objects.requireNonNull(sessions, "sessions"), Objects.requireNonNull(mapper, "mapper"),
            requireTimeout(timeoutMs), Objects.requireNonNull(executor, "executor"), null);
    }

    public static AndroidHttpTransport create(PublishableConfiguration configuration, SessionTokenProvider sessions) {
        ExecutorService service = Executors.newFixedThreadPool(4, runnable -> {
            Thread thread = new Thread(runnable, "layerx-android-transport");
            thread.setDaemon(true);
            return thread;
        });
        return new AndroidHttpTransport(configuration.serviceUri(), Objects.requireNonNull(sessions, "sessions"),
            new ObjectMapper(), requireTimeout((int) configuration.requestTimeoutMs()), service, service);
    }

    @Override
    public <T> CompletionStage<T> call(Call call, JavaType responseType) {
        Objects.requireNonNull(call, "call");
        Objects.requireNonNull(responseType, "responseType");
        if (call.operation().plane() != OperationCatalog.Plane.HUMAN) {
            return CompletableFuture.failedFuture(new PlatformSdkException(
                PlatformSdkException.Code.UNAVAILABLE_CAPABILITY, PlatformSdkException.Retry.NEVER, null, null, null));
        }
        OperationCatalog.requireKnown(call.operation().plane(), call.operation().wireName());
        OperationCatalog.Route route = OperationCatalog.HUMAN_ROUTES.get(call.operation().wireName());
        CompletableFuture<T> completion = new CompletableFuture<>();
        executor.execute(() -> {
            try {
                completion.complete(exchange(route, call, responseType));
            } catch (PlatformSdkException | MobileIntegrationException error) {
                completion.completeExceptionally(error);
            } catch (RuntimeException error) {
                completion.completeExceptionally(new PlatformSdkException(
                    PlatformSdkException.Code.TRANSPORT_FAILURE, PlatformSdkException.Retry.SAFE, null, null, null));
            }
        });
        return completion;
    }

    @Override
    public void close() {
        if (owned != null) owned.shutdownNow();
    }

    private <T> T exchange(OperationCatalog.Route route, Call call, JavaType responseType) {
        byte[] body = route.bodyless() ? null : encode(call.request());
        HttpURLConnection connection = null;
        try {
            connection = (HttpURLConnection) new URL(resolve(route, call).toString()).openConnection();
            method(connection, route.method());
            connection.setConnectTimeout(timeoutMs);
            connection.setReadTimeout(timeoutMs);
            connection.setInstanceFollowRedirects(false);
            connection.setUseCaches(false);
            connection.setRequestProperty("Accept", "application/json");
            connection.setRequestProperty("Content-Type", "application/json");
            connection.setRequestProperty("User-Agent", "layerx-android/0.1.0");
            if (call.idempotencyKey() != null) {
                connection.setRequestProperty("Idempotency-Key", call.idempotencyKey().value());
            }
            sessions.token().authorize(connection);
            if (body != null) {
                connection.setDoOutput(true);
                connection.setFixedLengthStreamingMode(body.length);
                try (OutputStream stream = connection.getOutputStream()) {
                    stream.write(body);
                }
            }
            int status = connection.getResponseCode();
            byte[] encoded = read(connection, status);
            return decode(status, encoded, responseType);
        } catch (IOException error) {
            throw new PlatformSdkException(PlatformSdkException.Code.TRANSPORT_FAILURE,
                PlatformSdkException.Retry.SAFE, null, null, null);
        } finally {
            if (connection != null) connection.disconnect();
        }
    }

    private byte[] read(HttpURLConnection connection, int status) throws IOException {
        InputStream stream = status >= 400 ? connection.getErrorStream() : connection.getInputStream();
        if (stream == null) {
            throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, null, null, null);
        }
        try (InputStream body = stream) {
            byte[] encoded = body.readNBytes(MAXIMUM_RESPONSE_BYTES + 1);
            if (encoded.length > MAXIMUM_RESPONSE_BYTES) {
                throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                    PlatformSdkException.Retry.NEVER, null, null, null);
            }
            return encoded;
        }
    }

    private URI resolve(OperationCatalog.Route route, Call call) {
        String path = route.path();
        for (String parameter : route.pathParameters()) {
            String value = call.pathParameters().require(parameter);
            path = path.replace("{" + parameter + "}", encodePath(value));
        }
        String prefix = humanBaseUri.toString();
        while (prefix.endsWith("/")) prefix = prefix.substring(0, prefix.length() - 1);
        return URI.create(prefix + path);
    }

    private byte[] encode(Object request) {
        try {
            byte[] encoded = mapper.writeValueAsBytes(request == null ? Map.of() : request);
            if (encoded.length > MAXIMUM_REQUEST_BYTES) throw PlatformSdkException.invalidArgument();
            return encoded;
        } catch (IOException error) {
            throw PlatformSdkException.invalidArgument();
        }
    }

    private <T> T decode(int status, byte[] encoded, JavaType type) {
        try {
            JsonNode envelope = mapper.readTree(encoded);
            if (envelope == null || !envelope.isObject() || !envelope.path("ok").isBoolean()) {
                throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                    PlatformSdkException.Retry.NEVER, null, null, null);
            }
            String trace = envelope.path("trace").isTextual() ? envelope.path("trace").textValue() : null;
            if (!envelope.path("ok").booleanValue()) throw serviceError(status, trace, envelope.path("error"));
            if (status < 200 || status >= 300 || envelope.has("error") || !envelope.has("result")) {
                throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                    PlatformSdkException.Retry.NEVER, trace, null, null);
            }
            return mapper.convertValue(envelope.get("result"), type);
        } catch (PlatformSdkException error) {
            throw error;
        } catch (IOException | IllegalArgumentException error) {
            throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, null, null, null);
        }
    }

    private static PlatformSdkException serviceError(int status, String trace, JsonNode error) {
        String code = error.path("code").asText("");
        PlatformSdkException.Code mapped = switch (code) {
            case "rate-limited" -> PlatformSdkException.Code.RATE_LIMIT;
            case "conflict" -> PlatformSdkException.Code.IDEMPOTENCY_CONFLICT;
            case "refused-by-policy" -> PlatformSdkException.Code.POLICY_REFUSAL;
            case "refused-by-budget" -> PlatformSdkException.Code.BUDGET_REFUSAL;
            case "refused-by-capability", "forbidden" -> PlatformSdkException.Code.CAPABILITY_REFUSAL;
            case "refused-by-protocol", "refused-by-limit" -> PlatformSdkException.Code.CORE_REJECTION;
            case "unavailable", "upstream-degraded" -> PlatformSdkException.Code.UNAVAILABLE_CAPABILITY;
            default -> status >= 500 ? PlatformSdkException.Code.INTERNAL_FAULT : PlatformSdkException.Code.CORE_REJECTION;
        };
        PlatformSdkException.Retry retry = switch (error.path("retry").asText("")) {
            case "retriable" -> PlatformSdkException.Retry.SAFE;
            case "retriable-after" -> PlatformSdkException.Retry.AFTER;
            case "structural", "final" -> PlatformSdkException.Retry.NEVER;
            default -> throw new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
                PlatformSdkException.Retry.NEVER, trace, null, null);
        };
        Long after = error.path("retry_after_ms").canConvertToLong() ? error.path("retry_after_ms").longValue() : null;
        Integer resultCode = error.path("protocol_result_code").canConvertToInt()
            ? error.path("protocol_result_code").intValue() : null;
        return new PlatformSdkException(mapped, retry, trace, resultCode, after);
    }

    private static void method(HttpURLConnection connection, String method) {
        try {
            connection.setRequestMethod(method.toUpperCase(Locale.ROOT));
        } catch (ProtocolException error) {
            throw new PlatformSdkException(PlatformSdkException.Code.UNAVAILABLE_CAPABILITY,
                PlatformSdkException.Retry.NEVER, null, null, null);
        }
    }

    private static int requireTimeout(int timeoutMs) {
        if (timeoutMs < 1_000 || timeoutMs > 300_000) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        return timeoutMs;
    }

    private static String encodePath(String value) {
        return URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20");
    }
}
