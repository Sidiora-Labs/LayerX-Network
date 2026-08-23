/**
 * Production LayerX SDK for Java and Kotlin.
 *
 * <p>This SDK provides wire-identical, idiomatic JVM support for the LayerX agent and human APIs
 * with local verification, production transport, and conformance-tested behavior.</p>
 *
 * <h2>Core Components</h2>
 * <ul>
 *   <li>{@link com.sidiora.layerx.sdk.ProductionClient} - Async CompletionStage-based client</li>
 *   <li>{@link com.sidiora.layerx.sdk.HttpProductionTransport} - HTTP/2 transport with authentication</li>
 *   <li>{@link com.sidiora.layerx.sdk.verify.LocalVerifier} - Trustless receipt and proof verification</li>
 *   <li>{@link com.sidiora.layerx.sdk.ResumableStream} - Virtual-thread streaming with cursors</li>
 * </ul>
 *
 * <h2>Protocol Types</h2>
 * <ul>
 *   <li>{@link com.sidiora.layerx.sdk.ProtocolAmount} - BigInteger-backed integer-only money</li>
 *   <li>{@link com.sidiora.layerx.sdk.IdempotencyKey} - Validated replay-safe keys</li>
 *   <li>{@link com.sidiora.layerx.sdk.SecretBytes} - Zeroizing secret container</li>
 * </ul>
 *
 * <h2>Error Handling</h2>
 * <p>All errors are represented as {@link com.sidiora.layerx.sdk.PlatformSdkException} with
 * wire-identical error codes and retry classification.</p>
 *
 * <h2>Kotlin Support</h2>
 * <p>Import {@code com.sidiora.layerx.sdk.LayerXKotlin} for idiomatic extension functions:
 * {@code protocolAmount()}, {@code idempotencyKey()}, {@code streamCursor()}.</p>
 *
 * <h2>Maven Coordinates</h2>
 * <pre>{@code
 * <dependency>
 *   <groupId>com.sidiora.layerx</groupId>
 *   <artifactId>layerx-sdk</artifactId>
 *   <version>0.1.0</version>
 * </dependency>
 * }</pre>
 *
 * @see <a href="https://docs.layerx.network/sdk/jvm">JVM SDK Documentation</a>
 */
package com.sidiora.layerx.sdk;
