# Seller middleware

`@sidiora/layerx-seller-middleware` is the piece that turns an HTTP resource into a paid one. The framework integrations for [Express](framework-express.html), [Next.js](framework-next.html), [FastAPI](framework-fastapi.html) and [Spring Boot](framework-spring.html) are thin mounts over it. Use it directly when your framework is not one of those, or when you want the decision without the routing.

## The shape

`SellerMiddleware` has exactly two entry points.

| Method | Returns |
|---|---|
| `paymentRequired()` | The `402` decision and its `PAYMENT-REQUIRED` header, for a request that carried no payment |
| `handle(...)` | One of four decisions for a request that did |

The four decisions are total. There is no fallthrough.

| Decision | HTTP | Meaning |
|---|---|---|
| `payment-required` | `402` | Pay this, exactly this |
| `pending` | `202` | The payment is real and still settling. Not a guess, not a failure |
| `refused` | The refusal | The payment was refused, and the settlement response says why |
| `released` | `200` | The receipt verified. Here is the resource, and here is the settlement header |

## What `handle` actually does

1. Decodes the `PAYMENT-SIGNATURE` header into a payment payload.
2. Matches the payload's accepted requirements against your declared requirements. A mismatch is `requirements-mismatch`, not a discount.
3. Asks the payment authority to settle. `ReceiptPayloadAuthority` extracts the receipt evidence carried in the payload and verifies it against an authorised batch resolved for that payment.
4. On a verified settlement, computes the request digest and hands it to your fulfilment repository together with a `release` callback. The repository decides whether to call it.
5. Returns the released resource and the `PAYMENT-RESPONSE` header.

Your resource handler runs at step 4 and only at step 4. There is no code path that releases the resource before verification.

## The two things you supply

**The authorised batch resolver.** Verification is meaningless if the facts you verify against come from the same party that gave you the receipt. `staticAuthorizedBatches` is right for a single-batch test; in production resolve the batch from a source you trust independently.

**The fulfilment repository.** This is where exactly-once lives:

- Same idempotency key, same request digest: return the stored fulfilment. Do not call `release` again.
- Same idempotency key, different request digest: raise `fulfillment-conflict`.
- New key: call `release`, store the result durably, return it.

An in-memory implementation is correct and not durable. On restart it forgets, and a buyer who already paid gets charged again or gets nothing. Both are your bug, not the protocol's.

## Errors

`MiddlewareError` carries one of a closed set of codes: `invalid-payment-required`, `invalid-payment-payload`, `requirements-mismatch`, `unsupported-payment`, `payment-pending`, `payment-refused`, `verification-failure`, `fulfillment-conflict`, `invalid-webhook`, `webhook-replay`. Map them to status codes once, centrally.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | The authority verifies receipt bytes against a batch header, with no LayerX Network service in the path. |
| Atomic settlement | `protocol` | A verified receipt describes a payment that happened whole. |
| Receipt-gated resource release | `service` | Binds requests that reach your service through this middleware, and nothing else. |
| Exactly-once fulfilment | `service` | As durable as the repository you supply. |
