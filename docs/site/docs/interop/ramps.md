# Ramps and fiat rails

Third-party on and off ramps are market makers operating ordinary LayerX
accounts. The protocol grants them no special authority. Default human
surfaces must not present them as LayerX custody
(`spec/layerx-platform/spec.kvx`, decision `multichain_surface` and
requirement 34 `ac_5`–`ac_6`).

Paxeer remains the sole LayerX custody and guaranteed-withdrawal boundary.

## Market-maker toolkit

`platform/ramps/README.md` describes the shipped toolkit and reference
service. `RampOrder::bind` combines an authenticated identity-plane customer
with an operator-owned quote. The LayerX legs are ordinary `LxpSend`
(operator to customer, on-ramp) and `LxpReceive` under the customer's payer
grant (customer to operator, off-ramp). Unknown submission is retained and
resolved only by activity receipt lookup.

The public status vocabulary is `pending`, `unknown`, `refused`,
`manual_review`, `reversed`, and `done`. `done` requires a verified LayerX
receipt and, for off-ramp, a settled external payout. A later provider
reversal removes `done` immediately.

The reference executable serves TLS on a configured address. Routes named in
that README are `POST /v1/orders`, `GET /v1/orders/{order_digest}`,
`POST /v1/provider-callbacks`, `/livez`, and `/readyz`, plus operator-only
`/internal/v1/work` and rebalance paths. Those are the reference ramp's
routes, not public LayerX Network product hostnames.

Customer JSON cannot set a customer account, operator account, compliance
result, provider credential, signer handle, or activity id.

## Fiat adapter

`interop/crates/layerx-fiat` is the gateway adapter id `fiat`. Supported
rails are `Card`, `Bank`, and `RealTimePayment`. Card data terminates at the
certified provider; the crate accepts only opaque token references
(`interop/crates/layerx-fiat/src/lib.rs`; `spec/layerx-platform/spec.kvx`,
requirement 33 `ac_3`–`ac_4`).

Money entering LayerX through an external rail is credited only against
verified settlement evidence. Chargeback and reversal are typed states, never
hidden (`spec/layerx-platform/spec.kvx`, decision `external_settlement`).

## Crash recovery (beta)

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 9) requires
a ramp callback to be validated completely against a staged view before any
durable append, and appended together with its index mutations in one step.
That repair is specified as a beta fix.

## Status

The toolkit, reference binary, and fiat adapter are in this tree. The
repository does not invent provider, compliance, or Paxeer custody-owner wire
coordinates (`platform/ramps/README.md`). Qualification against real provider
sandboxes is an owner-supplied beta input
(`spec/layerx-beta/spec.kvx`, decision `owner_inputs`).
