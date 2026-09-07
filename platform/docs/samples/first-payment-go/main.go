package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"time"

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
	QuoteID    string `json:"quote_id"`
	Money      Money  `json:"money"`
	FeeCeiling Money  `json:"fee_ceiling"`
	ExpiresAt  string `json:"expires_at"`
}

type MoveCommitRequest struct {
	QuoteID string `json:"quote_id"`
}

type EvidenceRef struct {
	EvidenceID   string `json:"evidence_id"`
	Class        string `json:"class"`
	Verification string `json:"verification"`
}

type Refusal struct {
	RefusedBy string `json:"refused_by"`
	MoneyLeft bool   `json:"money_left"`
}

type Journey struct {
	JourneyID string        `json:"journey_id"`
	Kind      string        `json:"kind"`
	State     string        `json:"state"`
	Evidence  []EvidenceRef `json:"evidence"`
	Refusal   *Refusal      `json:"refusal"`
}

type Report struct {
	JourneyID string   `json:"journey_id"`
	State     string   `json:"state"`
	Receipts  []string `json:"receipts"`
	RefusedBy string   `json:"refused_by,omitempty"`
	MoneyLeft *bool    `json:"money_left,omitempty"`
}

func main() {
	ctx := context.Background()
	apiURL := required("LAYERX_API_URL")
	apiToken := required("LAYERX_API_TOKEN")
	source := required("LAYERX_SOURCE")
	destination := required("LAYERX_DESTINATION")
	money := Money{Amount: required("LAYERX_AMOUNT"), Currency: required("LAYERX_CURRENCY")}
	key, err := layerx.NewIdempotencyKey(required("LAYERX_PAYMENT_KEY"))
	exitOn(err)

	// layerx:begin integration
	authorize := func(request *http.Request) error { request.Header.Set("Authorization", "Bearer "+apiToken); return nil }
	transport, err := layerx.NewHumanHTTPTransport(apiURL, nil, authorize)
	exitOn(err)
	client, err := layerx.NewClient(transport, nil)
	exitOn(err)
	var quote MoveQuote
	exitOn(client.Human(ctx, layerx.HumanOperationMoveQuote, MoveQuoteRequest{Source: source, Destination: destination, Money: money}, &quote, layerx.CallOptions{}))
	var journey Journey
	exitOn(client.Human(ctx, layerx.HumanOperationMoveCommit, MoveCommitRequest{QuoteID: quote.QuoteID}, &journey, layerx.CallOptions{IdempotencyKey: key}))
	// layerx:end integration

	for attempt := 0; attempt < 40 && !settled(journey.State); attempt++ {
		time.Sleep(250 * time.Millisecond)
		exitOn(client.Human(ctx, layerx.HumanOperationJourneyGet, struct{}{}, &journey, layerx.CallOptions{
			PathParameters: map[string]string{"journey_id": journey.JourneyID},
		}))
	}

	report := Report{JourneyID: journey.JourneyID, State: journey.State, Receipts: []string{}}
	for _, evidence := range journey.Evidence {
		if evidence.Class == "layerx-receipt" {
			report.Receipts = append(report.Receipts, evidence.EvidenceID)
		}
	}
	if journey.Refusal != nil {
		report.RefusedBy = journey.Refusal.RefusedBy
		report.MoneyLeft = &journey.Refusal.MoneyLeft
	}
	encoded, err := json.Marshal(report)
	exitOn(err)
	fmt.Fprintf(os.Stdout, "%s\n", encoded)
	if journey.State != "done" && journey.State != "done-finalised" {
		os.Exit(2)
	}
}

func settled(state string) bool {
	return state == "done" || state == "done-finalised" || state == "refused"
}

func required(name string) string {
	value := os.Getenv(name)
	if value == "" {
		fmt.Fprintf(os.Stderr, "first-payment-go: missing %s\n", name)
		os.Exit(1)
	}
	return value
}

func exitOn(err error) {
	if err != nil {
		fmt.Fprintf(os.Stderr, "first-payment-go: %v\n", err)
		os.Exit(1)
	}
}
