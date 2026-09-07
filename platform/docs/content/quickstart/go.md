# Go quickstart

Add a payment to a Go service. Nine lines, no protocol vocabulary, no key handling.

## Before you start

```text
go get github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go
```

| Variable | What it is |
|---|---|
| `LAYERX_API_URL` | The base URL of your environment |
| `LAYERX_API_TOKEN` | A bearer token identifying your account |

## The integration

```go sample=first-payment-go
authorize := func(request *http.Request) error { request.Header.Set("Authorization", "Bearer "+apiToken); return nil }
transport, err := layerx.NewHumanHTTPTransport(apiURL, nil, authorize)
exitOn(err)
client, err := layerx.NewClient(transport, nil)
exitOn(err)
var quote MoveQuote
exitOn(client.Human(ctx, layerx.HumanOperationMoveQuote, MoveQuoteRequest{Source: source, Destination: destination, Money: money}, &quote, layerx.CallOptions{}))
var journey Journey
exitOn(client.Human(ctx, layerx.HumanOperationMoveCommit, MoveCommitRequest{QuoteID: quote.QuoteID}, &journey, layerx.CallOptions{IdempotencyKey: key}))
```

The Go SDK ships its own human-plane HTTP transport, so the only thing you supply is how to authorise a request. The closure here sets a bearer header; if your deployment mints credentials per request, put that logic in the same place.

`layerx.CallOptions{IdempotencyKey: key}` is required on `HumanOperationMoveCommit`. Omit it and `Client.Human` refuses locally rather than sending an unprotected mutation.

## Run the whole sample

```text
cd platform/docs/samples/first-payment-go
go mod download
LAYERX_API_URL=http://127.0.0.1:9402 LAYERX_API_TOKEN=$(cat ./token) LAYERX_SOURCE=did:layerx:alice LAYERX_DESTINATION=did:layerx:bob \
LAYERX_AMOUNT=1250000 LAYERX_CURRENCY=USD LAYERX_PAYMENT_KEY=order-2f9c1b7e4a10 go run .
```

The sample declares its own request and response structs so the compiler checks the shapes you care about, polls `HumanOperationJourneyGet` with a path parameter until the journey settles, and prints a report.

## Why the count is nine and not four

Go has no exceptions, so `exitOn(err)` appears after each fallible call. That is your error policy, not LayerX code, which is why it sits outside the measured region along with your struct definitions. The [samples page](reference-samples.html) states the counting rule for every language.

## Handling refusals

Errors come back typed. Use `errors.As` to recover the SDK error and branch on its code rather than on a string.

| Code | What to do |
|---|---|
| `idempotency-required` | You omitted `CallOptions.IdempotencyKey` |
| `idempotency-conflict` | Same key, different body |
| `rate-limit` | Wait for the carried retry timing |
| `unknown-outcome` | Do not retry. Resolve by receipt lookup under your key |
| `budget-refusal` | The payment exceeded a funded budget |

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Atomic settlement | `protocol` | The committed move applies completely or not at all. |
| Replay refusal | `protocol` | A retried submission cannot apply twice. |
| Idempotent money moves | `service` | Repeating the commit with the same key returns the original journey. |
| Quote then commit | `service` | The quote you committed is the one that executes. |
| Done means verified | `service` | The `done` state is backed by receipt evidence. |
