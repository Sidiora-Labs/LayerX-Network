# Go SDK quickstart

Module: `github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go`
at `platform/sdk/go`. There is no public JSON-RPC client in this
module.

---

## Register

No `lx_register` wrapper. The agent catalog constant
`AgentOperationAgentRegister` (`"agent.register"`) can be sent
with `Client.Agent` **if** you supply a `Transport` that implements
the agent plane. This module ships `NewHumanHTTPTransport` only
(`platform/sdk/go/http.go`). There is no shipped agent-plane HTTP
transport. Calling `agent.register` through the human transport will
not reach the public gateway method.

Use [Public JSON-RPC](PublicRpc.md) or the Rust SDK.

---

## Fund from faucet

Same limitation. `AgentOperationFaucetClaim` (`"faucet.claim"`)
exists on the catalog. `NewHumanHTTPTransport` does not implement
the agent plane. Use [Hosted faucet](HostedFaucet.md) or
`lx_requestFunds` as documented on [Public JSON-RPC](PublicRpc.md).

---

## Send

The shipped send path is the human-plane quote then commit, as in
`platform/docs/samples/first-payment-go`:

```go
package main

import (
	"context"
	"net/http"

	layerx "github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go"
)

type Money struct {
	Amount   string `json:"amount"`
	Currency string `json:"currency"`
}

type MoveQuoteRequest struct {
	Source      string `json:"source"`
	Destination string `json:"destination"`
	Money       Money  `json:"money"`
}

type MoveQuote struct {
	QuoteID string `json:"quote_id"`
}

type MoveCommitRequest struct {
	QuoteID string `json:"quote_id"`
}

type Journey struct {
	JourneyID string `json:"journey_id"`
	State     string `json:"state"`
}

func pay(ctx context.Context, apiURL, apiToken, source, destination, paymentKey string, money Money) (Journey, error) {
	authorize := func(request *http.Request) error {
		request.Header.Set("Authorization", "Bearer "+apiToken)
		return nil
	}
	transport, err := layerx.NewHumanHTTPTransport(apiURL, nil, authorize)
	if err != nil {
		return Journey{}, err
	}
	client, err := layerx.NewClient(transport, nil)
	if err != nil {
		return Journey{}, err
	}
	key, err := layerx.NewIdempotencyKey(paymentKey)
	if err != nil {
		return Journey{}, err
	}
	var quote MoveQuote
	if err := client.Human(ctx, layerx.HumanOperationMoveQuote, MoveQuoteRequest{Source: source, Destination: destination, Money: money}, &quote, layerx.CallOptions{}); err != nil {
		return Journey{}, err
	}
	var journey Journey
	if err := client.Human(ctx, layerx.HumanOperationMoveCommit, MoveCommitRequest{QuoteID: quote.QuoteID}, &journey, layerx.CallOptions{IdempotencyKey: key}); err != nil {
		return Journey{}, err
	}
	return journey, nil
}
```

Agent `prepare` / `submit` constants exist
(`AgentOperationPrepare`, `AgentOperationSubmit`) and need an
agent-plane `Transport` this module does not ship.

---

## Verify a receipt

```go
package main

import layerx "github.com/Sidiora-Labs/LayerX-Network/platform/sdk/go"

func verify(canonicalReceipt []byte, authorized layerx.AuthorizedBatch) (layerx.VerifiedReceipt, error) {
	return layerx.VerifyReceipt(canonicalReceipt, authorized)
}
```

`AuthorizedBatch` is five `[32]byte` fields: `BatchID`, `Asset`,
`PreviousStateRoot`, `ResultingStateRoot`, `SequencerPublicKey`.
Those facts must come from a source you already trust.
`VerifyReceiptOutcome` returns the decoded outcome without requiring
result code 0.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
