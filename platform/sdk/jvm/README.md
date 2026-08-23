# LayerX JVM SDK

Production Java and Kotlin SDK for the LayerX agent and human APIs.

## Features

- **Schema-driven generation**: Generated from agent-api and human-api schemas
- **Wire-identical**: Cross-language parity with Go, TypeScript, Python, Swift, and C# SDKs
- **Local verification**: Trustless receipt, batch, Merkle, and checkpoint verification
- **Virtual-thread streaming**: Resumable cursors with atomic page acceptance
- **Integer-only money**: BigInteger-backed `ProtocolAmount` with no floating-point API
- **Security primitives**: `IdempotencyKey` for replay safety, `SecretBytes` for secret hygiene
- **Kotlin-friendly**: Extension functions for idiomatic Kotlin usage

## Requirements

- Java 21 or later
- Kotlin 2.0.20 or later (optional)

## Installation

Maven:

```xml
<dependency>
  <groupId>com.sidiora.layerx</groupId>
  <artifactId>layerx-sdk</artifactId>
  <version>0.1.0</version>
</dependency>
```

Gradle (Kotlin DSL):

```kotlin
implementation("com.sidiora.layerx:layerx-sdk:0.1.0")
```

## Quick Start

### Java

```java
import com.sidiora.layerx.sdk.*;
import java.net.URI;
import java.util.concurrent.CompletionStage;

var credential = new HttpProductionTransport.BearerCredential(
    new SecretBytes("your-api-key".getBytes()));
var transport = HttpProductionTransport.create(
    URI.create("https://api.layerx.network"),
    URI.create("https://agent.layerx.network/rpc"),
    credential);
var client = new ProductionClient(transport);

var options = ProductionClient.Options.idempotent(
    new IdempotencyKey("unique-key-123"));
CompletionStage<Map<String, Object>> response = client.agent(
    "version", 
    Map.of(), 
    new TypeReference<Map<String, Object>>() {},
    options);
```

### Kotlin

```kotlin
import com.sidiora.layerx.sdk.*

val credential = HttpProductionTransport.BearerCredential(
    SecretBytes("your-api-key".toByteArray()))
val transport = HttpProductionTransport.create(
    URI.create("https://api.layerx.network"),
    URI.create("https://agent.layerx.network/rpc"),
    credential)
val client = ProductionClient(transport)

val options = idempotencyKey("unique-key-123").asOptions()
val response = client.agent(
    "version",
    emptyMap(),
    Map::class,
    options)
```

## Local Verification

```java
import com.sidiora.layerx.sdk.verify.LocalVerifier;

byte[] canonicalReceipt = /* from API response */;
var authorized = new LocalVerifier.AuthorizedReceiptBatch(
    batchId, asset, previousStateRoot, resultingStateRoot, sequencerPublicKey);
var verified = LocalVerifier.verifyReceipt(canonicalReceipt, authorized);

System.out.println("Receipt verified: " + verified.level());
```

## Streaming

```java
import com.sidiora.layerx.sdk.ResumableStream;
import java.util.concurrent.Flow;

var stream = new ResumableStream<Event>(new ResumableStream.Cursor("initial"));
Flow.Publisher<ResumableStream.Event<Event>> publisher = stream.publisher(cursor ->
    client.human("stream.next", Map.of("cursor", cursor), EventPage.class,
        ProductionClient.Options.none()));

publisher.subscribe(new Flow.Subscriber<>() {
    public void onNext(ResumableStream.Event<Event> event) {
        System.out.println("Event: " + event.value());
    }
    // ... other methods
});
```

## Money Types

```java
import com.sidiora.layerx.sdk.ProtocolAmount;
import java.math.BigInteger;

// Integer-only amounts
ProtocolAmount amount = ProtocolAmount.of(new BigInteger("1000000"));
ProtocolAmount parsed = ProtocolAmount.parse("1000000");

// No floating-point API - this won't compile:
// ProtocolAmount invalid = ProtocolAmount.of(100.50); // Compilation error
```

## Error Handling

```java
try {
    var result = client.agent("operation", request, ResponseType.class, options)
        .toCompletableFuture().join();
} catch (PlatformSdkException e) {
    System.err.println("Code: " + e.code().wire());
    System.err.println("Retry: " + e.retry().wire());
    if (e.retryAfterMs() != null) {
        System.err.println("Retry after: " + e.retryAfterMs() + "ms");
    }
}
```

## Conformance

This SDK is conformance-tested against golden vectors and validates:

- Idempotency headers on mutating operations
- Integer-only money representation (no JSON numbers for amounts)
- Wire-identical error taxonomy across all platform SDKs
- Receipt and proof verification against protocol test vectors

Run conformance: `sh platform/sdk/conformance/run-jvm.sh`

## License

Proprietary - © 2026 Sidiora Labs
