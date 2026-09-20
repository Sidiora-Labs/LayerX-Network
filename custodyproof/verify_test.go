package custodyproof

import (
	"os"
	"testing"
	"time"

	consensuscrypto "github.com/sidiora-labs/paxeer-network/consensus/crypto"
)

func realRequest(t *testing.T) *Request {
	t.Helper()
	encoded, err := os.ReadFile("../../tests/fixtures/custody/paxeer-state-v2/state-credit.json")
	if err != nil {
		t.Fatal(err)
	}
	request, err := Decode(encoded)
	if err != nil {
		t.Fatal(err)
	}
	return request
}

func TestRealStateCreditAndClockBounds(t *testing.T) {
	request := realRequest(t)
	last := request.Bundle.History[len(request.Bundle.History)-1].Commit.Time
	result, err := Verify(request, last.Add(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	if result.DepositID != request.Expected.DepositID || result.StateHeight != request.Bundle.StateHeight ||
		result.FinalizedHeight != request.Bundle.FinalizedHeight {
		t.Fatal("verified state identity")
	}
	if _, err := Verify(request, last.Add(TrustingPeriod)); err == nil {
		t.Fatal("expired live head accepted")
	}
	if _, err := Verify(request, last.Add(-MaxClockDrift)); err == nil {
		t.Fatal("future header accepted")
	}
}

func TestRealStateCreditRefusesMissingAuthority(t *testing.T) {
	request := realRequest(t)
	now := request.Bundle.History[len(request.Bundle.History)-1].Commit.Time.Add(time.Second)
	request.Bundle.History[0].Validators[0].Validators[0].PubKey = consensuscrypto.PubKey{}
	if _, err := Verify(request, now); err == nil {
		t.Fatal("missing validator key accepted")
	}
	request = realRequest(t)
	request.Bundle.History[0].Commit.Commit = nil
	if _, err := Verify(request, now); err == nil {
		t.Fatal("missing commit accepted")
	}
}

func TestJSONRequestRefusesAmbiguousFields(t *testing.T) {
	for _, encoded := range []string{`{"expected":{},"expected":{},"bundle":{}}`,
		`{"expected":{},"bundle":{},"verified":true}`, `{} {}`} {
		if _, err := Decode([]byte(encoded)); err == nil {
			t.Fatal("ambiguous request accepted")
		}
	}
}
