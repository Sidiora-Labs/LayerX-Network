package layerx

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestSignedTerminalV4Vectors(t *testing.T) {
	for _, name := range []string{"executed-v4", "principal-v4", "mutated-leg-v4", "executed-v3", "account-bound-v4"} {
		t.Run(name, func(t *testing.T) {
			path := "../conformance/fixtures/receipt-programs-" + name + ".json"
			if name == "account-bound-v4" {
				path = "../../../programs/fixtures/pay5/receipt-account-bound-v4.json"
			}
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			var vector struct {
				Canonical string                  `json:"canonical_receipt_hex"`
				Activity  string                  `json:"signed_activity_hex"`
				Program   string                  `json:"program_id_hex"`
				Digest    string                  `json:"receipt_digest_hex"`
				Terminal  string                  `json:"terminal_payload_hex"`
				Graph     string                  `json:"call_graph_hex"`
				Authority receiptFixtureAuthority `json:"authorized_batch"`
			}
			if err := json.Unmarshal(raw, &vector); err != nil {
				t.Fatal(err)
			}
			authority := AuthorizedBatch{
				BatchID: fixture32(t, vector.Authority.BatchIDHex), Asset: fixture32(t, vector.Authority.AssetHex),
				PreviousStateRoot: fixture32(t, vector.Authority.PreviousStateRootHex), ResultingStateRoot: fixture32(t, vector.Authority.ResultingStateRootHex),
				SequencerPublicKey: fixture32(t, vector.Authority.SequencerPublicKeyHex),
			}
			verified, err := VerifyReceiptOutcome(fixtureBytes(t, vector.Canonical), authority, 3)
			if err != nil {
				t.Fatal(err)
			}
			if verified.ReceiptDigest != fixture32(t, vector.Digest) || verified.Receipt.ActivityID != domainDigest([]byte("LXP/v1/activity-id\x00"), fixtureBytes(t, vector.Activity)) {
				t.Fatal("signed vector identity mismatch")
			}
			outcome := verified.Receipt.ProgramOutcome
			if outcome == nil {
				t.Fatal("missing outcome")
			}
			terminal, graph := fixtureBytes(t, vector.Terminal), fixtureBytes(t, vector.Graph)
			inner, err := unwrapAppliedProgramTerminal(terminal, *outcome)
			if name == "mutated-leg-v4" {
				if err == nil || !strings.Contains(err.Error(), "applied transfer root") {
					t.Fatalf("mutated leg: %v", err)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			projection, err := decodeProgramTerminal(outcome.TerminalKind, outcome.ABIVersion, inner, fixture32(t, vector.Program), outcome.ResultCode)
			if err != nil {
				t.Fatal(err)
			}
			execution := ProgramExecutionDocument{
				ActivityID: hex.EncodeToString(verified.Receipt.ActivityID[:]), ProgramID: vector.Program,
				ModuleVersion: verified.Receipt.ModuleVersion, GuestABIVersion: outcome.ABIVersion,
				Receipt: vector.Canonical, ReceiptDigest: vector.Digest, TerminalPayload: vector.Terminal, CallGraph: vector.Graph,
				ResultCode: outcome.ResultCode, Outcome: projection.Outcome,
			}
			result, err := VerifyProgramReceipt(execution, authority, 3)
			if err != nil {
				t.Fatal(err)
			}
			expected := ProgramTransfersReconstructed
			if name == "executed-v3" {
				expected = ProgramTransfersRecorded
			}
			if result.TransferVerification != expected {
				t.Fatalf("status %q", result.TransferVerification)
			}
			if name == "executed-v4" || name == "account-bound-v4" {
				for length := 0; length < len(terminal); length++ {
					if _, err := verifyProgramTerminal(execution, verified.Receipt, terminal[:length], graph); err == nil {
						t.Fatalf("accepted truncation %d", length)
					}
				}
				if _, err := verifyProgramTerminal(execution, verified.Receipt, append(append([]byte{}, terminal...), 0), graph); err == nil {
					t.Fatal("accepted trailing byte")
				}
			}
		})
	}
}

func TestAppliedEmptyLegsRequireZeroRoot(t *testing.T) {
	encoded := append([]byte("LXP/programs/terminal-applied-legs/v1\x00"), 0, 0, 0, 1, 1, 0, 0, 0, 0)
	outcome := ProgramReceiptOutcome{EncodingVersion: 4, AppliedLegsDigest: sha256.Sum256(nil)}
	if _, err := unwrapAppliedProgramTerminal(encoded, outcome); err != nil {
		t.Fatal(err)
	}
	outcome.TransferRoot[0] = 1
	if _, err := unwrapAppliedProgramTerminal(encoded, outcome); err == nil {
		t.Fatal("accepted nonzero empty root")
	}
}

func TestNativeAccountAuthorizationVectors(t *testing.T) {
	raw, err := os.ReadFile("../../../programs/fixtures/pay5/account-authorization-vectors.json")
	if err != nil {
		t.Fatal(err)
	}
	var vectors []struct {
		Name    string
		Encoded string
		Root    string
		Accept  bool
	}
	if err := json.Unmarshal(raw, &vectors); err != nil {
		t.Fatal(err)
	}
	for _, v := range vectors {
		err := verifyProgramTransferAuthorization(fixtureBytes(t, v.Encoded), fixture32(t, v.Root))
		if (err == nil) != v.Accept {
			t.Fatalf("%s: accept=%v error=%v", v.Name, v.Accept, err)
		}
	}
}
