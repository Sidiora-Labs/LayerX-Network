package com.sidiora.layerx.android.sample;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.android.LayerXAndroid;
import com.sidiora.layerx.android.MobileIntegrationException;
import com.sidiora.layerx.android.ReceiptGate;
import com.sidiora.layerx.android.VerifiedEventConsumer;
import com.sidiora.layerx.sdk.IdempotencyKey;
import com.sidiora.layerx.sdk.GeneratedSchema.HumanOperations;
import com.sidiora.layerx.sdk.PlatformSdkException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletionException;

/** Application state driven entirely by verified service answers. */
public final class WalletModel {
    public record Snapshot(String serviceVersion, String displayName, int activityCount,
                           ReceiptGate.State settlement, List<String> deliveries, String refusal) {
        public Snapshot { deliveries = List.copyOf(deliveries); }
        public static Snapshot empty() { return new Snapshot("", "", 0, null, List.of(), null); }
    }

    @FunctionalInterface public interface Backoff {
        void pause(long millis) throws InterruptedException;
    }

    private final LayerXAndroid mobile;
    private final ReceiptGate gate;
    private final ObjectMapper mapper;
    private final List<String> deliveries = new ArrayList<>();
    private Snapshot snapshot = Snapshot.empty();

    public WalletModel(LayerXAndroid mobile, ReceiptGate.ReceiptResolver receipts) {
        this.mobile = Objects.requireNonNull(mobile, "mobile");
        this.gate = mobile.gate(Objects.requireNonNull(receipts, "receipts"));
        this.mapper = mobile.mapper();
    }

    public synchronized Snapshot current() { return snapshot; }

    public synchronized Snapshot refresh() {
        try {
            HumanOperations.VersionResponse version = await(mobile.client().versionTyped());
            HumanOperations.ProfileGetResponse profile = await(mobile.client().profileTyped());
            HumanOperations.ActivityQueryResponse activity = await(mobile.client().activity(
                new HumanOperations.ActivityQueryRequest(null, null, 25L)));
            snapshot = new Snapshot(
                version.service(),
                profile.display_name(),
                activity.groups().stream().mapToInt(group -> group.entries().size()).sum(),
                snapshot.settlement(),
                deliveries,
                null);
        } catch (RuntimeException error) {
            snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                snapshot.settlement(), deliveries, refusal(error));
        }
        return snapshot;
    }

    public synchronized Snapshot pay(JsonNode quoteRequest, ReceiptGate.Expectation expectation, IdempotencyKey key) {
        try {
            HumanOperations.MoveQuoteRequest request = mapper.convertValue(
                quoteRequest, HumanOperations.MoveQuoteRequest.class);
            HumanOperations.MoveQuoteResponse quote = await(mobile.client().quote(request));
            HumanOperations.MoveCommitResponse committed = await(mobile.client().commit(
                new HumanOperations.MoveCommitRequest(quote.quote_id()), key));
            JsonNode journey = mapper.valueToTree(committed);
            snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                gate.project(journey, expectation), deliveries, null);
        } catch (RuntimeException error) {
            snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                null, deliveries, refusal(error));
        }
        return snapshot;
    }

    public synchronized Snapshot awaitSettlement(String journeyId, ReceiptGate.Expectation expectation,
                                                 int attempts, Backoff backoff) {
        for (int attempt = 0; attempt < Math.max(attempts, 1); attempt++) {
            try {
                JsonNode journey = mapper.valueToTree(await(mobile.client().journeyTyped(journeyId)));
                ReceiptGate.State state = gate.project(journey, expectation);
                snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                    state, deliveries, null);
                if (!(state instanceof ReceiptGate.Pending)) return snapshot;
                backoff.pause(Math.min(attempt + 1, 10) * 250L);
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                return snapshot;
            } catch (RuntimeException error) {
                snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                    snapshot.settlement(), deliveries, refusal(error));
                return snapshot;
            }
        }
        return snapshot;
    }

    public synchronized Snapshot deliver(byte[] rawBody, Map<String, String> headerFields) {
        try {
            VerifiedEventConsumer.Outcome outcome = mobile.consume(rawBody, headerFields, this::record);
            if (outcome != VerifiedEventConsumer.Outcome.PROCESSED) {
                append(outcome.name().toLowerCase(java.util.Locale.ROOT));
            }
            snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                snapshot.settlement(), deliveries, null);
        } catch (RuntimeException error) {
            snapshot = new Snapshot(snapshot.serviceVersion(), snapshot.displayName(), snapshot.activityCount(),
                snapshot.settlement(), deliveries, refusal(error));
        }
        return snapshot;
    }

    private void record(JsonNode event, String deliveryId) {
        String kind = text(event, "type", "kind");
        append(deliveryId + ":" + (kind.isEmpty() ? "event" : kind));
    }

    private void append(String entry) {
        deliveries.add(entry);
        while (deliveries.size() > 64) deliveries.remove(0);
    }

    private static String text(JsonNode event, String... fields) {
        for (String field : fields) {
            JsonNode value = event.path(field);
            if (value.isTextual() && !value.textValue().isEmpty()) return value.textValue();
        }
        return "";
    }

    private static <T> T await(java.util.concurrent.CompletionStage<T> stage) {
        try {
            return stage.toCompletableFuture().join();
        } catch (CompletionException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException runtime) throw runtime;
            throw error;
        }
    }

    private static String refusal(RuntimeException error) {
        if (error instanceof MobileIntegrationException mobile) return mobile.code().wire();
        if (error instanceof PlatformSdkException sdk) return sdk.code().wire();
        return "transport-failure";
    }
}
