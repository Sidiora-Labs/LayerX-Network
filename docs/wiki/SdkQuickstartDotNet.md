# .NET SDK quickstart

NuGet: `LayerX.Sdk` at `platform/sdk/dotnet`. There is no public
JSON-RPC client in this package.

---

## Register

No `lx_register` wrapper. Generated
`PlatformClient.AgentAgentRegisterAsync` /
`MutateAsync(PlatformOperation.AgentAgentRegister, …)` sends
`"agent.register"`. `HumanHttpTransport` is the human plane. Use
[Public JSON-RPC](PublicRpc.md) or the Rust SDK for self-service
principal creation.

---

## Fund from faucet

No `lx_requestFunds` wrapper. `AgentFaucetClaimAsync` /
`ReadAsync(PlatformOperation.AgentFaucetClaim, …)` exists on the
catalog and needs an agent-plane `IPlatformTransport`. Use
[Hosted faucet](HostedFaucet.md) or `lx_requestFunds` as documented
on [Public JSON-RPC](PublicRpc.md).

---

## Send

Human-plane quote then commit, as in
`platform/docs/samples/first-payment-csharp`:

```csharp
using System.Text;
using LayerX.Sdk;

static async Task<JsonValue> Pay(
    Uri apiUrl,
    string apiToken,
    string source,
    string destination,
    JsonValue money,
    string paymentKey)
{
    using var token = new AccessToken(Encoding.UTF8.GetBytes(apiToken));
    var layerx = new PlatformClient(new HumanHttpTransport(apiUrl, accessToken: token));
    var quote = await layerx.HumanMoveQuoteAsync(JsonValue.Object(new Dictionary<string, JsonValue>
    {
        ["source"] = JsonValue.String(source),
        ["destination"] = JsonValue.String(destination),
        ["money"] = money,
    }));
    IReadOnlyDictionary<string, JsonValue> fields = quote is JsonValue.ObjectValue record
        ? record.Value
        : new Dictionary<string, JsonValue>();
    string quoteId = fields.TryGetValue("quote_id", out var found) && found is JsonValue.StringValue text
        ? text.Value
        : string.Empty;
    return await layerx.HumanMoveCommitAsync(
        JsonValue.Object(new Dictionary<string, JsonValue> { ["quote_id"] = JsonValue.String(quoteId) }),
        new IdempotencyKey(paymentKey));
}
```

The SDK calls that exist are `HumanMoveQuoteAsync` and
`HumanMoveCommitAsync`. Agent `prepare` / `submit` need an
appropriate `IPlatformTransport`.

---

## Verify a receipt

```csharp
using LayerX.Sdk;

static ValueTask<ReceiptVerification> Verify(
    ReadOnlyMemory<byte> canonicalReceipt,
    AuthorizedReceiptBatch authorized) =>
    LocalVerifier.VerifyReceiptAsync(canonicalReceipt, authorized);
```

`AuthorizedReceiptBatch` is
`(byte[] BatchId, byte[] Asset, byte[] PreviousStateRoot, byte[] ResultingStateRoot, byte[] SequencerPublicKey)`.
Those facts must come from a source you already trust.
`VerifyReceiptOutcomeAsync` returns the decoded outcome without
requiring result code 0. Default `protocolVersion` is `2`.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
