# TypeScript SDK quickstart

Package: `@sidiora/layerx-sdk` at `agent/sdk/typescript`.
`JsonRpcClient` is exported from `src/rpc.ts`. Offline verification
is `verifyReceipt` in `src/verifier.ts`. The hosted human plane is
`ProductionClient` in `src/production.ts`.

---

## Register

`JsonRpcClient` has no `register` method. Its private `call` is not
part of the public surface, so this package cannot invoke
`lx_register`.

The generated agent operation `"agent.register"` exists on
`Client.call` and `ProductionClient.agent`. That is daemon/agent
registration, not public self-service principal creation.
`AgentHttpTransport` only routes `program.*` operations; calling
`agent.register` through it returns `unavailable-capability`.

Use [Public JSON-RPC](PublicRpc.md) `lx_register` over HTTPS, or the
Rust SDK's `RpcClient::register`.

---

## Fund from faucet

`JsonRpcClient` has no `requestFunds` method. The generated
operation `"faucet.claim"` exists on the agent catalog and has the
same transport limitation as `agent.register`.

Use [Hosted faucet](HostedFaucet.md) `POST /v1/faucet/claims` with a
Bearer session, or `lx_requestFunds` as documented on
[Public JSON-RPC](PublicRpc.md).

---

## Send

Public JSON-RPC send that exists on this package:

```ts
import { JsonRpcClient, LayerXKeyCredential, SecretBytes } from "@sidiora/layerx-sdk";
import type { Commitment } from "@sidiora/layerx-sdk";

export function openRpc(endpoint: string, keyId: string, secret: Uint8Array): JsonRpcClient {
  return new JsonRpcClient(endpoint, new LayerXKeyCredential(keyId, new SecretBytes(secret)));
}

export function send(
  rpc: JsonRpcClient,
  canonical: Uint8Array,
  commitment: Commitment = "executed",
): Promise<Record<string, unknown>> {
  return rpc.sendActivity(canonical, commitment);
}
```

`JsonRpcClient` also exposes `getReceipt`, `getActivityStatus`,
`getAccount`, `getBalance`, `getBalances`, `getSequence`,
`getIdentitySequence`, `getBatchHeader`, `getCheckpoint`,
`getNodeInfo`, `listAssets`, `getAsset`, `estimateFee`, `getProof`,
`subscribe`, and `subscribeFrom`. It does not wrap `lx_register` or
`lx_requestFunds`.

Hosted human-plane send (quote then commit), as in
`platform/docs/samples/first-payment-typescript`:

```ts
import { ProductionClient, SecretBytes, idempotencyKey } from "@sidiora/layerx-sdk";

export async function pay(
  layerx: ProductionClient,
  source: string,
  destination: string,
  money: { amount: string; currency: string },
  paymentKey: string,
) {
  const quote = await layerx.human("move.quote", { source, destination, money });
  return layerx.human("move.commit", { quote_id: quote.quote_id }, { idempotencyKey: idempotencyKey(paymentKey) });
}
```

`ProductionClient` needs a `ProductionTransport`. The in-repo sample
uses `@sidiora/layerx-buyer-middleware`'s
`LayerXPaymentHttpTransport`. That transport is not defined inside
`@sidiora/layerx-sdk`.

---

## Verify a receipt

```ts
import { verifyReceipt } from "@sidiora/layerx-sdk";
import type { AuthorizedReceiptBatch, ReceiptVerification } from "@sidiora/layerx-sdk";

export function verify(
  canonicalReceipt: Uint8Array,
  authorized: AuthorizedReceiptBatch,
): Promise<ReceiptVerification> {
  return verifyReceipt(canonicalReceipt, authorized);
}
```

`AuthorizedReceiptBatch` is `{ batchId, asset, previousStateRoot, resultingStateRoot, sequencerPublicKey }`, each a 32-byte
`Uint8Array`. Those five facts must come from a source you already
trust. `verifyReceipt` refuses a non-zero `resultCode`;
`verifyReceiptOutcome` returns the decoded outcome without that
check.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
