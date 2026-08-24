package com.sidiora.layerx.android;

import android.content.Context;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.sdk.PlatformSdk;
import com.sidiora.layerx.sdk.ProductionClient;
import java.nio.file.Path;
import java.security.KeyFactory;
import java.security.NoSuchAlgorithmException;
import java.security.Security;
import java.security.Signature;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import org.bouncycastle.jce.provider.BouncyCastleProvider;

/** The single entry point an Android application configures, holding no credential of its own. */
public final class LayerXAndroid implements AutoCloseable {
    public static final String NAME = "com.sidiora.layerx:layerx-android";
    public static final String VERSION = "0.1.0";

    private final PublishableConfiguration configuration;
    private final BrokeredSessionTokenProvider sessions;
    private final AndroidHttpTransport transport;
    private final MobileClient client;
    private final VerifiedEventConsumer events;
    private final ObjectMapper mapper;

    private LayerXAndroid(PublishableConfiguration configuration, BrokeredSessionTokenProvider sessions,
                          AndroidHttpTransport transport, MobileClient client, VerifiedEventConsumer events,
                          ObjectMapper mapper) {
        this.configuration = configuration;
        this.sessions = sessions;
        this.transport = transport;
        this.client = client;
        this.events = events;
        this.mapper = mapper;
    }

    public static LayerXAndroid create(PublishableConfiguration configuration) {
        return create(configuration, new FileEventDeliveryStore(FileEventDeliveryStore.defaultPath()));
    }

    public static LayerXAndroid create(Context context, PublishableConfiguration configuration) {
        Objects.requireNonNull(context, "context");
        return create(configuration,
            context.getFilesDir().toPath().resolve("layerx-event-deliveries-v1.json"));
    }

    public static LayerXAndroid create(PublishableConfiguration configuration, Path deliveryStorePath) {
        return create(configuration, new FileEventDeliveryStore(deliveryStorePath));
    }

    public static LayerXAndroid create(PublishableConfiguration configuration, EventDeliveryStore deliveries) {
        Objects.requireNonNull(configuration, "configuration");
        installSignatureProvider();
        ObjectMapper mapper = new ObjectMapper();
        BrokeredSessionTokenProvider sessions = BrokeredSessionTokenProvider.create(configuration);
        AndroidHttpTransport transport = AndroidHttpTransport.create(configuration, sessions);
        MobileClient client = new MobileClient(new ProductionClient(transport, mapper, null), sessions, mapper);
        VerifiedEventConsumer events = VerifiedEventConsumer.create(configuration, deliveries);
        return new LayerXAndroid(configuration, sessions, transport, client, events, mapper);
    }

    public static LayerXAndroid ofDeclaredKeys(Map<String, String> declaredKeys) {
        return create(PublishableConfiguration.of(declaredKeys));
    }

    public static LayerXAndroid ofJsonFile(Path path) {
        return create(PublishableConfiguration.ofJsonFile(path));
    }

    public PublishableConfiguration configuration() { return configuration; }
    public MobileClient client() { return client; }
    public VerifiedEventConsumer events() { return events; }
    public SessionTokenProvider sessions() { return sessions; }
    public ObjectMapper mapper() { return mapper; }

    public ReceiptGate gate(ReceiptGate.ReceiptResolver receipts) {
        return new ReceiptGate(receipts);
    }

    public VerifiedEventConsumer.Outcome consume(byte[] rawBody, Map<String, String> headerFields,
                                                 VerifiedEventConsumer.Handler handler) {
        return events.consume(rawBody, EventEnvelopeHeaders.of(headerFields), handler);
    }

    @Override
    public void close() {
        try {
            transport.close();
        } finally {
            sessions.close();
        }
    }

    static void installSignatureProvider() {
        try {
            KeyFactory.getInstance("Ed25519");
            Signature.getInstance("Ed25519");
        } catch (NoSuchAlgorithmException absent) {
            Security.removeProvider(BouncyCastleProvider.PROVIDER_NAME);
            Security.insertProviderAt(new BouncyCastleProvider(), 1);
        }
    }

    /** Stable Codify and runtime package identity. */
    public static Map<String, Object> platform_int_android() {
        Map<String, Object> metadata = new LinkedHashMap<>();
        metadata.put("name", NAME);
        metadata.put("version", VERSION);
        metadata.put("sdk", PlatformSdk.platform_sdk_jvm());
        metadata.put("credentialModel", "brokered-ephemeral-session-token");
        metadata.put("eventVerification", "ed25519-v1");
        metadata.put("replayProtection", "durable-leased-delivery-claim");
        metadata.put("declaredKeys", PublishableConfiguration.declaredKeyNames());
        return Map.copyOf(metadata);
    }
}
