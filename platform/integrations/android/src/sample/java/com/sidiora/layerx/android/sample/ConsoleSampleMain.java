package com.sidiora.layerx.android.sample;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.android.EventEnvelopeHeaders;
import com.sidiora.layerx.android.LayerXAndroid;
import com.sidiora.layerx.android.MobileIntegrationException;
import com.sidiora.layerx.android.PublishableConfiguration;
import com.sidiora.layerx.android.ReceiptGate;
import com.sidiora.layerx.sdk.IdempotencyKey;
import com.sidiora.layerx.sdk.ProtocolAmount;
import java.io.IOException;
import java.math.BigInteger;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Base64;
import java.util.Map;

/** End-to-end driver: brokered session, real move, device-side receipt verification, verified event replay. */
public final class ConsoleSampleMain {
    private record SignedWebhookDelivery(String body, Map<String, String> headers) {}

    private ConsoleSampleMain() {}

    public static void main(String[] arguments) {
        Map<String, String> environment = System.getenv();
        ObjectMapper mapper = new ObjectMapper();
        ObjectNode report = mapper.createObjectNode();

        PublishableConfiguration configuration;
        try {
            configuration = SampleEnvironment.configuration(environment);
        } catch (MobileIntegrationException error) {
            fail("configuration refused: " + error.code().wire());
            return;
        }

        try (LayerXAndroid mobile = LayerXAndroid.create(configuration)) {
            RelayReceiptResolver receipts = new RelayReceiptResolver(
                URI.create(SampleEnvironment.required(environment, "LAYERX_RECEIPT_RELAY_URL")),
                mapper, (int) configuration.requestTimeoutMs());
            WalletModel model = new WalletModel(mobile, receipts);

            WalletModel.Snapshot refreshed = model.refresh();
            report.put("service_version", refreshed.serviceVersion());
            report.put("activity_count", refreshed.activityCount());
            if (refreshed.refusal() != null) {
                report.put("refusal", refreshed.refusal());
                emit(report);
                System.exit(3);
                return;
            }

            ReceiptGate.Expectation expectation = new ReceiptGate.Expectation(
                RelayReceiptResolver.hex32(SampleEnvironment.required(environment, "LAYERX_SAMPLE_ASSET")),
                RelayReceiptResolver.hex32(SampleEnvironment.required(environment, "LAYERX_SAMPLE_RECIPIENT")),
                amount(SampleEnvironment.required(environment, "LAYERX_SAMPLE_AMOUNT")));
            IdempotencyKey key = new IdempotencyKey(
                SampleEnvironment.required(environment, "LAYERX_SAMPLE_IDEMPOTENCY_KEY"));
            JsonNode quoteRequest = json(mapper, SampleEnvironment.required(environment, "LAYERX_SAMPLE_QUOTE_JSON"));

            WalletModel.Snapshot paid = model.pay(quoteRequest, expectation, key);
            boolean verified = record(report, paid);
            if (paid.settlement() instanceof ReceiptGate.Pending pending) {
                WalletModel.Snapshot settled = model.awaitSettlement(pending.reference(), expectation, 20, Thread::sleep);
                verified = record(report, settled);
            }

            String deliveryPath = environment.get("LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH");
            if (deliveryPath != null && !deliveryPath.isEmpty()) {
                SignedWebhookDelivery delivery = signedWebhookDelivery(mapper, Path.of(deliveryPath));
                byte[] body;
                try {
                    body = Base64.getDecoder().decode(delivery.body());
                } catch (IllegalArgumentException error) {
                    throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
                }
                if (body.length == 0) throw MobileIntegrationException.of(
                    MobileIntegrationException.Code.INVALID_CONFIGURATION);
                byte[] tampered = body.clone();
                tampered[tampered.length - 1] ^= 1;
                WalletModel.Snapshot rejected = model.deliver(tampered, delivery.headers());
                if (!MobileIntegrationException.Code.INVALID_EVENT.wire().equals(rejected.refusal())) {
                    throw MobileIntegrationException.of(MobileIntegrationException.Code.VERIFICATION_FAILURE);
                }
                WalletModel.Snapshot first = model.deliver(body, delivery.headers());
                if (first.refusal() != null) {
                    report.put("event", "refused");
                    report.put("refusal", first.refusal());
                    emit(report);
                    System.exit(4);
                    return;
                }
                WalletModel.Snapshot replayed = model.deliver(body, delivery.headers());
                report.put("event_tamper", "rejected");
                report.put("event", "verified");
                report.put("event_replay", replayed.deliveries().isEmpty()
                    ? "duplicate" : replayed.deliveries().get(replayed.deliveries().size() - 1));
            } else if ((deliveryPath = environment.get("LAYERX_SAMPLE_EVENT_PATH")) != null
                    && !deliveryPath.isEmpty()) {
                String eventPath = deliveryPath;
                byte[] body = read(Path.of(eventPath));
                Map<String, String> headers = Map.of(
                    EventEnvelopeHeaders.ID_HEADER, SampleEnvironment.required(environment, "LAYERX_SAMPLE_EVENT_ID"),
                    EventEnvelopeHeaders.TIMESTAMP_HEADER,
                        SampleEnvironment.required(environment, "LAYERX_SAMPLE_EVENT_TIMESTAMP"),
                    EventEnvelopeHeaders.KEY_ID_HEADER,
                        SampleEnvironment.required(environment, "LAYERX_SAMPLE_EVENT_KEY_ID"),
                    EventEnvelopeHeaders.SIGNATURE_HEADER,
                        SampleEnvironment.required(environment, "LAYERX_SAMPLE_EVENT_SIGNATURE"));
                WalletModel.Snapshot first = model.deliver(body, headers);
                if (first.refusal() != null) {
                    report.put("event", "refused");
                    report.put("refusal", first.refusal());
                    emit(report);
                    System.exit(4);
                    return;
                }
                WalletModel.Snapshot replayed = model.deliver(body, headers);
                report.put("event", "verified");
                report.put("event_replay", replayed.deliveries().isEmpty()
                    ? "duplicate" : replayed.deliveries().get(replayed.deliveries().size() - 1));
            }

            report.put("integration", String.valueOf(LayerXAndroid.platform_int_android().get("name")));
            emit(report);
            System.exit(verified ? 0 : 5);
        } catch (MobileIntegrationException error) {
            fail("sample refused: " + error.code().wire());
        }
    }

    private static boolean record(ObjectNode report, WalletModel.Snapshot snapshot) {
        ReceiptGate.State state = snapshot.settlement();
        if (state instanceof ReceiptGate.Verified verified) {
            report.put("settlement", verified.level());
            report.put("receipt_digest", verified.receiptDigest());
            return true;
        }
        if (state instanceof ReceiptGate.Refused refused) {
            report.put("settlement", "refused");
            report.put("refusal", refused.code());
            return false;
        }
        if (state instanceof ReceiptGate.Pending) {
            report.put("settlement", "pending");
            return false;
        }
        report.put("settlement", "refused");
        report.put("refusal", snapshot.refusal() == null ? "verification-failure" : snapshot.refusal());
        return false;
    }

    private static ProtocolAmount amount(String value) {
        for (int index = 0; index < value.length(); index++) {
            if (value.charAt(index) < '0' || value.charAt(index) > '9') {
                throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
            }
        }
        if (value.isEmpty() || (value.length() > 1 && value.charAt(0) == '0')) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        return new ProtocolAmount(new BigInteger(value));
    }

    private static JsonNode json(ObjectMapper mapper, String value) {
        try {
            JsonNode parsed = mapper.readTree(value);
            if (parsed == null || !parsed.isObject()) {
                throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
            }
            return parsed;
        } catch (IOException error) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
        }
    }

    private static byte[] read(Path path) {
        try {
            return Files.readAllBytes(path);
        } catch (IOException error) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.DECODE_FAILURE);
        }
    }

    private static SignedWebhookDelivery signedWebhookDelivery(ObjectMapper mapper, Path path) {
        try {
            SignedWebhookDelivery delivery = mapper.readValue(Files.readAllBytes(path), SignedWebhookDelivery.class);
            if (delivery == null || delivery.body() == null || delivery.body().isEmpty()
                    || delivery.headers() == null) {
                throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
            }
            return delivery;
        } catch (IllegalArgumentException | IOException error) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
    }

    private static void emit(ObjectNode report) {
        try {
            System.out.println(new ObjectMapper().writeValueAsString(report));
        } catch (IOException error) {
            System.exit(2);
        }
    }

    private static void fail(String reason) {
        System.err.println("layerx-android-sample: " + reason);
        System.exit(2);
    }
}
