# SDK quickstarts

One page per shipped SDK. Each shows the real register, faucet, send,
and verify-receipt surface of that package — or states that a step
is not on that surface.

Do not conflate three different “register” / “fund” / “send” planes:

| Plane | What it is | Where it lives |
| --- | --- | --- |
| Public JSON-RPC | `lx_register`, `lx_requestFunds`, `lx_sendActivity` | [Public JSON-RPC](PublicRpc.md) |
| Agent operations | `agent.register`, `faucet.claim`, `prepare` / `submit` | generated agent catalogs |
| Human plane | `move.quote` / `move.commit` | [Human journeys](HumanJourneys.md) |

`agent.register` is daemon/agent registration, not the public
self-service principal flow.

| Language | Package | Register | Faucet | Send | Verify receipt |
| --- | --- | --- | --- | --- | --- |
| [Rust](SdkQuickstartRust.md) | `layerx-sdk` (`agent/crates/layerx-sdk`) | `RpcClient::register` | `RpcClient::request_funds` (see auth note) | `RpcClient::send_activity` | `production::verify_receipt` |
| [TypeScript](SdkQuickstartTypeScript.md) | `@sidiora/layerx-sdk` | not on `JsonRpcClient` | not on `JsonRpcClient` | `JsonRpcClient.sendActivity` or `ProductionClient.human` | `verifyReceipt` |
| [Python](SdkQuickstartPython.md) | `layerx-sdk` | `PaymentRpc.call("lx_register", …)` | `PaymentRpc.call("lx_requestFunds", …)` | `PaymentRpc.send` or `ProductionClient.human` | `verify_receipt` |
| [Go](SdkQuickstartGo.md) | `platform/sdk/go` | catalog only | catalog only | `Client.Human` `move.quote` / `move.commit` | `VerifyReceipt` |
| [JVM](SdkQuickstartJvm.md) | `com.sidiora.layerx:layerx-sdk` | catalog only | catalog only | `ProductionClient.human` | `LocalVerifier.verifyReceipt` |
| [Swift](SdkQuickstartSwift.md) | `LayerXSDK` | catalog only | catalog only | `humanMoveQuote` / `humanMoveCommit` | `LocalVerifier.verifyReceipt` |
| [.NET](SdkQuickstartDotNet.md) | `LayerX.Sdk` | catalog only | catalog only | `HumanMoveQuoteAsync` / `HumanMoveCommitAsync` | `LocalVerifier.VerifyReceiptAsync` |

There is no Rust tree under `platform/sdk`. There is no TypeScript or
Python tree under `platform/sdk`. Go, JVM, Swift, and .NET ship only
under `platform/sdk`.

`AgentHttpTransport` in TypeScript, Python, and Swift is
**programs-only**. Calling `agent.register`, `faucet.claim`,
`prepare`, or `submit` through it returns `unavailable-capability`.
Go has no shipped agent-plane HTTP transport.

[Home](Home.md)
