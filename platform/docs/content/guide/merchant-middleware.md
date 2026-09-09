# Merchant middleware

`@sidiora/layerx-merchant-middleware` is a checkout built on top of the seller middleware. It prices a cart, opens an order, gates the resource on payment, and reconciles late settlement webhooks against the order it already has.

## Pricing

`quote(checkoutKey, lines)` turns a cart into a `MerchantQuote` and the `PAYMENT-REQUIRED` offer that goes with it. It is strict on purpose:

| Condition | Code |
|---|---|
| Empty cart, more than 256 lines, duplicate SKU, non-positive or non-integer quantity | `invalid-cart` |
| A SKU the catalog does not have | `catalog-item-missing` |
| Two lines whose asset, `payTo`, scheme, network or `extra` differ | `mixed-payment-facts` |
| A total that would exceed the 128-bit amount range | `amount-overflow` |

Amounts are integer strings and are multiplied and summed as `bigint`. No line total is ever computed in floating point, and no rounding step exists to argue about.

`mixed-payment-facts` is the rule people are surprised by. A cart is one payment; a payment has one asset, one recipient and one `extra` (including any `extra.layerx.commitment`). If your catalog mixes them, split the cart into separate checkouts.

## Checking out

`checkout(principal, checkoutKey, lines, paymentHeader?)` returns one of four results.

| Result | What happened |
|---|---|
| `payment-required` | The order is open and unpaid. Send `decision.headers` and `402` |
| `pending` | The payment is real and still settling |
| `refused` | The payment was refused; the order is marked `refused` |
| `paid` | The receipt verified, the order is `paid-verified`, and it carries the receipt digest and transaction |

The order store is asked to `open` the order before the seller decision, and every store response is re-checked: if `open`, `markPaid` or `markRefused` returns an order whose `checkoutKey` or `requestDigest` does not match, the middleware raises `order-conflict` rather than continuing on a mismatched row. Your store cannot quietly swap the order under it.

The request digest is computed over the quote itself - lines, total, asset, recipient, scheme, network, timeout and the `paymentRequired` envelope, so a change to `extra` (including commitment) is a different order. Re-quoting the same cart against an unchanged catalog gives the same digest and therefore the same order. A catalog price change gives a different digest, so it is a different order, which is what you want.

## Late settlement

`MerchantSettlementWebhooks.consume(rawBody, headers)` handles the case where the payment lands after the HTTP exchange ended. It runs inside the verified webhook consumer, so signature, freshness and replay are already handled, and then:

1. Looks the order up by `order_id` and requires the event's `request_digest` to match the stored one.
2. Resolves the receipt reference to canonical receipt bytes and a batch authorisation.
3. Verifies the receipt against the offer's requirements.
4. Recomputes the receipt's Merkle leaf digest and compares it to the event's `receipt_digest` in constant time.
5. Only then marks the order paid.

A webhook body that says "paid" proves nothing here. The receipt does. An event carrying an unknown field, a non-hex digest or a verification level outside the five real ones is `invalid-webhook`.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | Both the HTTP path and the webhook path verify receipt bytes against a batch authorisation. |
| Conserved supply | `protocol` | The amount in the receipt is the amount that moved. |
| Quote then commit | `service` | The order is bound to the digest of the quote the buyer accepted. |
| Exactly-once fulfilment | `service` | Order state transitions are digest-checked on every store response. |
| Verified, replay-protected webhooks | `service` | Signature, freshness, replay and receipt digest, in that order. |
