package com.sidiora.layerx.sdk.examples

import com.sidiora.layerx.sdk.*
import java.net.URI
import java.util.concurrent.CompletionStage

/**
 * Kotlin example: Service lifecycle with budget constraints.
 * 
 * Demonstrates idiomatic Kotlin usage of the LayerX SDK.
 */
fun main() {
    val apiKey = System.getenv("LAYERX_API_KEY")
        ?: error("Set LAYERX_API_KEY environment variable")

    val credential = HttpProductionTransport.BearerCredential(
        SecretBytes(apiKey.toByteArray()))
    val transport = HttpProductionTransport.create(
        URI.create("https://api.layerx.network"),
        URI.create("https://agent.layerx.network/rpc"),
        credential)
    val client = ProductionClient(transport)

    val budgetAmount = protocolAmount("5000000")
    val createBudget = mapOf(
        "name" to "example-service-budget",
        "limit" to budgetAmount.toString(),
        "asset" to "USD")
    
    val budgetOptions = idempotencyKey("budget-${System.currentTimeMillis()}")
        .asOptions()
    
    val budgetResult: CompletionStage<Map<String, Any>> = client.agent(
        "budget.create",
        createBudget,
        Map::class,
        budgetOptions)

    budgetResult.thenAccept { result ->
        println("Budget created: ${result["budget_id"]}")
        println("Limit: ${result["limit"]}")
        println("Available: ${result["available"]}")
    }.toCompletableFuture().join()

    println("Service lifecycle setup complete")
}
