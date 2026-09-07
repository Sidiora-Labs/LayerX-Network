# Paying for things

Every payment on every LayerX Network surface has the same three steps. Learn them once.

1. **Quote.** You say who is paying, who is being paid and how much. You get back what will actually happen: the mechanism, the fee estimate with its ceiling, when the money is expected to arrive, and whether any part of it is irreversible.
2. **Commit.** You turn exactly that quote into a journey, carrying an idempotency key you chose.
3. **Watch the journey.** The journey moves through stages and ends in a state. Every claim it makes carries an evidence reference behind it.

Here is the whole thing in TypeScript. This is the measured region of a runnable sample, not an excerpt:

```js sample=first-payment-typescript
import { LayerXPaymentHttpTransport } from "@sidiora/layerx-buyer-middleware";
import { ProductionClient, SecretBytes, idempotencyKey } from "@sidiora/layerx-sdk";
export const openLayerX = (apiUrl, apiToken) => new ProductionClient(
  new LayerXPaymentHttpTransport({ baseUrl: apiUrl, bearerToken: new SecretBytes(new TextEncoder().encode(apiToken)) }));
export const pay = async (layerx, source, destination, money, paymentKey) => {
  const quote = await layerx.human("move.quote", { source, destination, money });
  return layerx.human("move.commit", { quote_id: quote.quote_id }, { idempotencyKey: idempotencyKey(paymentKey) });
};
```

## Why quote-then-commit and not just pay

Because "pay" would have to guess. The quote is where the surprises live - a fee ceiling, a slower rail, an irreversible leg - and committing a quote means you agreed to the specific thing the quote described. If the world changed underneath it, the quote expires and you get `quote-expired` rather than a different payment than the one you agreed to.

## Journey states

This vocabulary is fixed. A journey never reports a state outside it.

| State | Meaning |
|---|---|
| `getting-ready` | Accepted, preparing the legs |
| `sending` | At least one leg is in flight |
| `waiting-for-you` | Held for a human decision, with a deterministic expiry |
| `processing` | Submitted and settling |
| `still-checking` | The outcome is genuinely unknown and resolves by receipt lookup under your idempotency key |
| `done` | Settled, with a verified receipt or finality proof in the evidence |
| `done-finalised` | The receipt-backed outcome is additionally covered by a finalised checkpoint proof |
| `refused` | Refused, naming who refused and whether any money left the account |

Two of those deserve attention. `refused` always carries a `refusal` record with `refused_by` and a `money_left` boolean, so you never have to guess whether a failed payment cost anything. `still-checking` is not a spinner - it is an honest statement that the outcome is not yet knowable, and every control that could duplicate the payment stays locked while it lasts. See [Retries and unknown outcomes](concepts-idempotency.html).

## Evidence

A journey carries evidence references rather than raw protocol structures. A reference with class `layerx-receipt` points at a canonical receipt you can fetch and verify yourself; see [Receipts and verification](concepts-receipts.html).

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Quote then commit | `service` | `layerx-human-service` enforces the ordering and the quote's expiry. It binds callers of that service. |
| Idempotent money moves | `service` | Committing the same key twice returns the original journey rather than paying twice. |
| Done means verified | `service` | A journey reaches `done` only against verified evidence. |
| Atomic settlement | `protocol` | Each leg applies completely or not at all, whatever happens above the protocol. |
| Replay refusal | `protocol` | Even if the service were bypassed, an already-applied activity cannot apply again. |
