# JVM SDK quickstart

Maven coordinates: `com.sidiora.layerx:layerx-sdk` at
`platform/sdk/jvm`. There is no public JSON-RPC client in this
package.

The in-repo `platform/sdk/jvm/examples/FirstPaymentExample.java`
calls `client.agent("prepare", …)` against placeholder URIs. This
page uses the human-plane sample that actually ships a complete
quote/commit path:
`platform/docs/samples/first-payment-jvm`.

---

## Register

No `lx_register` wrapper. Catalog constant
`GeneratedSchema.AgentOperations.AGENT_REGISTER` (`"agent.register"`)
can be sent with `ProductionClient.agent` when
`HttpProductionTransport` is constructed with an agent RPC URL.
That operation is daemon/agent registration, not public
`lx_register`. Use [Public JSON-RPC](PublicRpc.md) or the Rust SDK
for self-service principal creation.

---

## Fund from faucet

No `lx_requestFunds` wrapper. `"faucet.claim"` exists on the agent
catalog and needs an agent-plane URL on
`HttpProductionTransport`. Use [Hosted faucet](HostedFaucet.md) or
`lx_requestFunds` as documented on [Public JSON-RPC](PublicRpc.md).

---

## Send

Human-plane quote then commit
(`platform/docs/samples/first-payment-jvm`):

```java
import com.fasterxml.jackson.databind.JsonNode;
import com.sidiora.layerx.sdk.HttpProductionTransport;
import com.sidiora.layerx.sdk.IdempotencyKey;
import com.sidiora.layerx.sdk.ProductionClient;
import com.sidiora.layerx.sdk.SecretBytes;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.Map;

public final class Pay {
    public static JsonNode pay(
        String apiUrl,
        String apiToken,
        String source,
        String destination,
        Map<String, String> money,
        String paymentKey
    ) {
        var credential = new HttpProductionTransport.BearerCredential(
            new SecretBytes(apiToken.getBytes(StandardCharsets.UTF_8)));
        var layerx = new ProductionClient(
            HttpProductionTransport.create(URI.create(apiUrl), URI.create(apiUrl), credential));
        var quote = layerx.human(
            "move.quote",
            Map.of("source", source, "destination", destination, "money", money),
            JsonNode.class,
            ProductionClient.Options.none()).toCompletableFuture().join();
        return layerx.human(
            "move.commit",
            Map.of("quote_id", quote.path("quote_id").asText()),
            JsonNode.class,
            ProductionClient.Options.idempotent(new IdempotencyKey(paymentKey)))
            .toCompletableFuture().join();
    }

    private Pay() {}
}
```

`HttpProductionTransport.create` takes a human URL and an agent URL.
The sample passes the same human origin twice. Agent `prepare` /
`submit` need a real agent RPC origin; this package does not invent
one.

---

## Verify a receipt

```java
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import com.sidiora.layerx.sdk.verify.LocalVerifier.AuthorizedReceiptBatch;
import com.sidiora.layerx.sdk.verify.LocalVerifier.ReceiptVerification;

public final class Verify {
    public static ReceiptVerification verify(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized) {
        return LocalVerifier.verifyReceipt(canonicalReceipt, authorized);
    }

    private Verify() {}
}
```

`AuthorizedReceiptBatch` is
`(byte[] batchId, byte[] asset, byte[] previousStateRoot, byte[] resultingStateRoot, byte[] sequencerPublicKey)`.
Those facts must come from a source you already trust.
`verifyReceiptOutcome` returns the decoded outcome without requiring
result code 0.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
