# Swift SDK quickstart

SwiftPM product: `LayerXSDK` at `platform/sdk/swift`. There is no
public JSON-RPC client in this package.

---

## Register

No `lx_register` wrapper. Generated
`PlatformClient.agentAgentRegister` / `mutate(.agentAgentRegister, …)`
sends `"agent:agent.register"`. `AgentHTTPTransport` is
programs-only. Use [Public JSON-RPC](PublicRpc.md) or the Rust SDK
for self-service principal creation.

---

## Fund from faucet

No `lx_requestFunds` wrapper. `agentFaucetClaim` /
`read(.agentFaucetClaim, …)` exists on the catalog and has the same
transport limitation. Use [Hosted faucet](HostedFaucet.md) or
`lx_requestFunds` as documented on [Public JSON-RPC](PublicRpc.md).

---

## Send

Human-plane quote then commit, as in
`platform/docs/samples/first-payment-swift`:

```swift
import Foundation
import LayerXSDK

func pay(
    serviceURL: URL,
    apiToken: String,
    source: String,
    destination: String,
    money: JSONValue,
    paymentKey: IdempotencyKey
) async throws -> JSONValue {
    let token = try AccessToken(Data(apiToken.utf8))
    let layerx = PlatformClient(transport: try HumanHTTPTransport(baseURL: serviceURL, accessToken: token))
    let quote = try await layerx.humanMoveQuote(.object([
        "source": .string(source),
        "destination": .string(destination),
        "money": money,
    ]))
    guard let quoteID = quote.objectValue?["quote_id"]?.stringValue else {
        throw PlatformSDKError(code: .invalidArgument, retry: .never)
    }
    return try await layerx.humanMoveCommit(.object(["quote_id": .string(quoteID)]), idempotencyKey: paymentKey)
}
```

If `PlatformSDKError.invalidArgument` is not the refusal the
package uses for a missing field, treat the `guard` as sample
control flow — the SDK call that exists is `humanMoveQuote` /
`humanMoveCommit`. Agent `prepare` / `submit` need a transport that
implements the agent plane.

---

## Verify a receipt

```swift
import Foundation
import LayerXSDK

func verify(canonicalReceipt: Data, authorized: AuthorizedReceiptBatch) async throws -> ReceiptVerification {
    try await LocalVerifier.verifyReceipt(canonicalReceipt, authorized: authorized)
}
```

`AuthorizedReceiptBatch` is
`(batchID: Data, asset: Data, previousStateRoot: Data, resultingStateRoot: Data, sequencerPublicKey: Data)`.
Those facts must come from a source you already trust.
`verifyReceiptOutcome` returns the decoded outcome without requiring
result code 0. Default `protocolVersion` is `2`.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
