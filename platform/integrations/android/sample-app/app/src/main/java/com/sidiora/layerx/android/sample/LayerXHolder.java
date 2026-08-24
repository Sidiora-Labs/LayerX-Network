package com.sidiora.layerx.android.sample;

import android.content.Context;
import android.content.res.Resources;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sidiora.layerx.android.LayerXAndroid;
import com.sidiora.layerx.android.MobileIntegrationException;
import com.sidiora.layerx.android.PublishableConfiguration;
import java.net.URI;
import java.util.HashMap;
import java.util.Map;

/** Holds the process-wide binding built from publishable resources only. */
public final class LayerXHolder {
    private static LayerXHolder instance;

    private final LayerXAndroid mobile;
    private final WalletModel model;
    private final String receiptRelayUrl;

    private LayerXHolder(LayerXAndroid mobile, WalletModel model, String receiptRelayUrl) {
        this.mobile = mobile;
        this.model = model;
        this.receiptRelayUrl = receiptRelayUrl;
    }

    public static synchronized LayerXHolder shared(Context context) {
        if (instance == null) instance = build(context.getApplicationContext());
        return instance;
    }

    public LayerXAndroid mobile() { return mobile; }
    public WalletModel model() { return model; }
    public String receiptRelayUrl() { return receiptRelayUrl; }

    private static LayerXHolder build(Context context) {
        Resources resources = context.getResources();
        String keyId = resources.getString(R.string.layerx_event_key_id);
        String publicKeyName = "layerx_event_public_key_" + keyId.replace('-', '_');
        int publicKeyId = resources.getIdentifier(publicKeyName, "string", context.getPackageName());
        if (publicKeyId == 0) {
            throw MobileIntegrationException.of(MobileIntegrationException.Code.INVALID_CONFIGURATION);
        }
        Map<String, String> declared = new HashMap<>();
        declared.put("layerx_service_url", resources.getString(R.string.layerx_service_url));
        declared.put("layerx_session_broker_url", resources.getString(R.string.layerx_session_broker_url));
        declared.put("layerx_event_max_age_seconds", resources.getString(R.string.layerx_event_max_age_seconds));
        declared.put("layerx_request_timeout_seconds", resources.getString(R.string.layerx_request_timeout_seconds));
        declared.put(publicKeyName, resources.getString(publicKeyId));

        PublishableConfiguration configuration = SampleEnvironment.configuration(declared);
        LayerXAndroid mobile = LayerXAndroid.create(context, configuration);
        String relay = resources.getString(R.string.layerx_receipt_relay_url);
        RelayReceiptResolver receipts = new RelayReceiptResolver(
            URI.create(relay), new ObjectMapper(), (int) configuration.requestTimeoutMs());
        return new LayerXHolder(mobile, new WalletModel(mobile, receipts), relay);
    }
}
