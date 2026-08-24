@file:JvmName("LayerXKotlin")

package com.sidiora.layerx.sdk

import com.fasterxml.jackson.databind.node.ObjectNode
import java.math.BigInteger
import java.util.concurrent.CompletionStage
import kotlin.reflect.KClass

public fun protocolAmount(value: BigInteger): ProtocolAmount = ProtocolAmount.of(value)
public fun protocolAmount(value: String): ProtocolAmount = ProtocolAmount.parse(value)
public fun idempotencyKey(value: String): IdempotencyKey = IdempotencyKey(value)
public fun streamCursor(value: String): ResumableStream.Cursor = ResumableStream.Cursor(value)
public fun agentOperation(wireName: String): SchemaTypes.AgentOperation = OperationCatalog.agent(wireName)
public fun humanOperation(wireName: String): SchemaTypes.HumanOperation = OperationCatalog.human(wireName)
public fun agentRequest(
    operation: SchemaTypes.AgentOperation,
    body: ObjectNode,
): SchemaTypes.AgentRequest = SchemaTypes.AgentRequest(operation, body)
public fun humanRequest(
    operation: SchemaTypes.HumanOperation,
    body: ObjectNode,
): SchemaTypes.HumanRequest = SchemaTypes.HumanRequest(operation, body)

public fun ProductionClient.agent(
    request: SchemaTypes.AgentRequest,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<SchemaTypes.AgentResponse> = agent(request, options)

public fun ProductionClient.human(
    request: SchemaTypes.HumanRequest,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<SchemaTypes.HumanResponse> = human(request, options)

public fun <R : SchemaTypes.GeneratedRequest, S : SchemaTypes.GeneratedResponse> ProductionClient.agent(
    operation: SchemaTypes.TypedOperation<R, S>,
    request: R,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<S> = agent(operation, request, options)

public fun <R : SchemaTypes.GeneratedRequest, S : SchemaTypes.GeneratedResponse> ProductionClient.human(
    operation: SchemaTypes.TypedOperation<R, S>,
    request: R,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<S> = human(operation, request, options)

public fun <T : Any> ProductionClient.agent(
    operation: String,
    request: Any,
    responseType: KClass<T>,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<T> = agent(operation, request, responseType.java, options)

public fun <T : Any> ProductionClient.human(
    operation: String,
    request: Any,
    responseType: KClass<T>,
    options: ProductionClient.Options = ProductionClient.Options.none(),
): CompletionStage<T> = human(operation, request, responseType.java, options)

public fun IdempotencyKey.asOptions(
    pathParameters: Map<String, String> = emptyMap(),
): ProductionClient.Options = ProductionClient.Options(this, pathParameters)
