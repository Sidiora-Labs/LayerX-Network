package com.sidiora.layerx.android;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.IdempotencyKey;
import com.sidiora.layerx.sdk.GeneratedSchema.HumanOperations;
import com.sidiora.layerx.sdk.PlatformSdkException;
import com.sidiora.layerx.sdk.ProductionClient;
import com.sidiora.layerx.sdk.ResumableStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.CompletionStage;
import java.util.function.Supplier;

/** The human-plane surface a mobile application is allowed to reach. */
public final class MobileClient {
    private final ProductionClient client;
    private final SessionTokenProvider sessions;
    private final ObjectMapper mapper;

    public MobileClient(ProductionClient client, SessionTokenProvider sessions, ObjectMapper mapper) {
        this.client = Objects.requireNonNull(client, "client");
        this.sessions = Objects.requireNonNull(sessions, "sessions");
        this.mapper = Objects.requireNonNull(mapper, "mapper");
    }

    public CompletionStage<JsonNode> version() {
        return authorized(() -> client.human("version", null, JsonNode.class, ProductionClient.Options.none()));
    }

    public CompletionStage<HumanOperations.VersionResponse> versionTyped() {
        return authorized(() -> client.human(HumanOperations.VERSION,
            new HumanOperations.VersionRequest(), ProductionClient.Options.none()));
    }

    public CompletionStage<JsonNode> profile() {
        return authorized(() -> client.human("profile.get", null, JsonNode.class, ProductionClient.Options.none()));
    }

    public CompletionStage<HumanOperations.ProfileGetResponse> profileTyped() {
        return authorized(() -> client.human(HumanOperations.PROFILE_GET,
            new HumanOperations.ProfileGetRequest(), ProductionClient.Options.none()));
    }

    public CompletionStage<JsonNode> activity(JsonNode request) {
        JsonNode body = request == null ? mapper.createObjectNode() : request;
        return authorized(() -> client.human("activity.query", body, JsonNode.class, ProductionClient.Options.none()));
    }

    public CompletionStage<HumanOperations.ActivityQueryResponse> activity(
            HumanOperations.ActivityQueryRequest request) {
        Objects.requireNonNull(request, "request");
        return authorized(() -> client.human(HumanOperations.ACTIVITY_QUERY, request,
            ProductionClient.Options.none()));
    }

    public CompletionStage<JsonNode> activityEntry(String entryId) {
        String value = pathValue(entryId);
        return authorized(() -> client.human("activity.entry", null, JsonNode.class,
            new ProductionClient.Options(null, Map.of("entry_id", value))));
    }

    public CompletionStage<HumanOperations.ActivityEntryResponse> activityEntryTyped(String entryId) {
        String value = pathValue(entryId);
        return authorized(() -> client.human(HumanOperations.ACTIVITY_ENTRY,
            new HumanOperations.ActivityEntryRequest(),
            new ProductionClient.Options(null, Map.of("entry_id", value))));
    }

    public CompletionStage<JsonNode> journeys() {
        return authorized(() -> client.human("journey.list", null, JsonNode.class, ProductionClient.Options.none()));
    }

    public CompletionStage<HumanOperations.JourneyListResponse> journeysTyped() {
        return authorized(() -> client.human(HumanOperations.JOURNEY_LIST,
            new HumanOperations.JourneyListRequest(), ProductionClient.Options.none()));
    }

    public CompletionStage<JsonNode> journey(String journeyId) {
        String value = pathValue(journeyId);
        return authorized(() -> client.human("journey.get", null, JsonNode.class,
            new ProductionClient.Options(null, Map.of("journey_id", value))));
    }

    public CompletionStage<HumanOperations.JourneyGetResponse> journeyTyped(String journeyId) {
        String value = pathValue(journeyId);
        return authorized(() -> client.human(HumanOperations.JOURNEY_GET,
            new HumanOperations.JourneyGetRequest(),
            new ProductionClient.Options(null, Map.of("journey_id", value))));
    }

    public CompletionStage<JsonNode> quote(JsonNode request) {
        Objects.requireNonNull(request, "request");
        return authorized(() -> client.human("move.quote", request, JsonNode.class, ProductionClient.Options.none()));
    }

    public CompletionStage<HumanOperations.MoveQuoteResponse> quote(HumanOperations.MoveQuoteRequest request) {
        Objects.requireNonNull(request, "request");
        return authorized(() -> client.human(HumanOperations.MOVE_QUOTE, request,
            ProductionClient.Options.none()));
    }

    public CompletionStage<JsonNode> commit(JsonNode request, IdempotencyKey idempotencyKey) {
        Objects.requireNonNull(request, "request");
        Objects.requireNonNull(idempotencyKey, "idempotencyKey");
        return authorized(() -> client.human("move.commit", request, JsonNode.class,
            ProductionClient.Options.idempotent(idempotencyKey)));
    }

    public CompletionStage<HumanOperations.MoveCommitResponse> commit(
            HumanOperations.MoveCommitRequest request, IdempotencyKey idempotencyKey) {
        Objects.requireNonNull(request, "request");
        Objects.requireNonNull(idempotencyKey, "idempotencyKey");
        return authorized(() -> client.human(HumanOperations.MOVE_COMMIT, request,
            ProductionClient.Options.idempotent(idempotencyKey)));
    }

    public CompletionStage<ResumableStream.Cursor> openStream() {
        return authorized(() -> client.human("stream.open", null, JsonNode.class, ProductionClient.Options.none()))
            .thenApply(response -> {
                if (!response.path("cursor").isTextual()) {
                    throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
                }
                return new ResumableStream.Cursor(response.path("cursor").textValue());
            });
    }

    public CompletionStage<HumanOperations.StreamOpenResponse> openStreamTyped() {
        return authorized(() -> client.human(HumanOperations.STREAM_OPEN,
            new HumanOperations.StreamOpenRequest(), ProductionClient.Options.none()));
    }

    public CompletionStage<ResumableStream.Page<JsonNode>> events(ResumableStream.Cursor cursor) {
        Objects.requireNonNull(cursor, "cursor");
        return authorized(() -> client.human("stream.next", null, JsonNode.class,
            new ProductionClient.Options(null, Map.of("cursor", cursor.value()))))
            .thenApply(response -> page(cursor, response));
    }

    public ResumableStream.PageSource<JsonNode> streamSource() {
        return this::events;
    }

    private ResumableStream.Page<JsonNode> page(ResumableStream.Cursor requested, JsonNode response) {
        JsonNode entries = response.path("events");
        if (!entries.isArray() || !response.path("next_cursor").isTextual()) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
        }
        List<ResumableStream.Event<JsonNode>> events = new ArrayList<>(entries.size());
        ResumableStream.Cursor previous = requested;
        for (JsonNode entry : entries) {
            if (!entry.isObject() || !entry.path("cursor").isTextual()) {
                throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
            }
            ResumableStream.Cursor advanced = new ResumableStream.Cursor(entry.path("cursor").textValue());
            events.add(new ResumableStream.Event<>(advanced.value(), previous, advanced, entry));
            previous = advanced;
        }
        return new ResumableStream.Page<>(requested, events,
            new ResumableStream.Cursor(response.path("next_cursor").textValue()));
    }

    private <T> CompletionStage<T> authorized(Supplier<CompletionStage<T>> operation) {
        return attempt(operation).handle((value, failure) -> {
            if (failure == null) return CompletableFuture.completedFuture(value);
            if (unwrap(failure) instanceof PlatformSdkException sdk
                    && sdk.code() == PlatformSdkException.Code.CAPABILITY_REFUSAL) {
                sessions.invalidate();
                return attempt(operation).toCompletableFuture();
            }
            return CompletableFuture.<T>failedFuture(unwrap(failure));
        }).thenCompose(stage -> stage);
    }

    private <T> CompletionStage<T> attempt(Supplier<CompletionStage<T>> operation) {
        try {
            return operation.get();
        } catch (RuntimeException error) {
            return CompletableFuture.failedFuture(error);
        }
    }

    private static Throwable unwrap(Throwable failure) {
        return failure instanceof CompletionException wrapped && wrapped.getCause() != null
            ? wrapped.getCause() : failure;
    }

    private static String pathValue(String value) {
        if (value == null || value.isEmpty() || value.getBytes(java.nio.charset.StandardCharsets.UTF_8).length > 255
                || value.indexOf('\0') >= 0 || value.indexOf('/') >= 0
                || value.indexOf('?') >= 0 || value.indexOf('#') >= 0) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        return value;
    }
}
