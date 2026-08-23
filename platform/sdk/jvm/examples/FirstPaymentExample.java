package com.sidiora.layerx.sdk.examples;

import com.sidiora.layerx.sdk.*;
import com.fasterxml.jackson.core.type.TypeReference;
import java.net.URI;
import java.util.Map;
import java.util.concurrent.CompletionStage;

/**
 * Example: First payment using the LayerX JVM SDK.
 * 
 * <p>Demonstrates the minimal integration to send a payment with verification.
 */
public final class FirstPaymentExample {
    public static void main(String[] args) throws Exception {
        String apiKey = System.getenv("LAYERX_API_KEY");
        if (apiKey == null) {
            System.err.println("Set LAYERX_API_KEY environment variable");
            System.exit(1);
        }

        var credential = new HttpProductionTransport.BearerCredential(
            new SecretBytes(apiKey.getBytes()));
        var transport = HttpProductionTransport.create(
            URI.create("https://api.layerx.network"),
            URI.create("https://agent.layerx.network/rpc"),
            credential);
        var client = new ProductionClient(transport);

        var prepareRequest = Map.of(
            "to", "did:layerx:example-recipient",
            "amount", "1000000",
            "asset", "USD");
        
        var prepareOptions = ProductionClient.Options.idempotent(
            new IdempotencyKey("payment-" + System.currentTimeMillis()));
        
        CompletionStage<Map<String, Object>> prepared = client.agent(
            "prepare",
            prepareRequest,
            new TypeReference<Map<String, Object>>() {},
            prepareOptions);

        prepared.thenAccept(result -> {
            System.out.println("Prepared activity: " + result.get("activity_id"));
            System.out.println("Fee: " + result.get("fee_charged"));
        }).toCompletableFuture().join();

        System.out.println("Payment prepared successfully");
    }
}
