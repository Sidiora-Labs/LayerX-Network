package layerx

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"testing"
)

func lifecycleFixture(t *testing.T, name string) (uint16, []byte, []byte, map[string]json.RawMessage) {
	t.Helper()
	raw, err := os.ReadFile("../conformance/fixtures/native-program-" + name + "-v3.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	var ordinal uint16
	if err := json.Unmarshal(fixture["ordinal"], &ordinal); err != nil {
		t.Fatal(err)
	}
	for field, expected := range map[string]uint16{"protocol_version": 3, "module": 9} {
		var value uint16
		if err := json.Unmarshal(fixture[field], &value); err != nil || value != expected {
			t.Fatalf("invalid %s", field)
		}
	}
	return ordinal, lifecycleFixtureBytes(t, fixture, "payload_hex"), lifecycleFixtureBytes(t, fixture, "signed_activity_hex"), fixture
}

func lifecycleFixtureBytes(t *testing.T, fixture map[string]json.RawMessage, field string) []byte {
	t.Helper()
	var value string
	if err := json.Unmarshal(fixture[field], &value); err != nil {
		t.Fatal(err)
	}
	bytes, err := hex.DecodeString(value)
	if err != nil {
		t.Fatal(err)
	}
	return bytes
}

func lifecycleReencode(ordinal uint16, payload []byte) ([]byte, error) {
	switch ordinal {
	case 1:
		value, err := DecodeNativeProgramDeploy(payload)
		if err != nil {
			return nil, err
		}
		return EncodeNativeProgramDeploy(value)
	case 2:
		value, err := DecodeNativeProgramUpgrade(payload)
		if err != nil {
			return nil, err
		}
		return EncodeNativeProgramUpgrade(value)
	case 7:
		value, err := DecodeNativeProgramWindDown(payload)
		if err != nil {
			return nil, err
		}
		return EncodeNativeProgramWindDown(value)
	default:
		return nil, lifecycleInvalid()
	}
}

func TestNativeLifecycleCFixtures(t *testing.T) {
	for _, name := range []string{"deploy", "upgrade", "wind-down-route", "wind-down-deprecate", "wind-down-tombstone", "wind-down-exit"} {
		t.Run(name, func(t *testing.T) {
			ordinal, payload, signed, fixture := lifecycleFixture(t, name)
			encoded, err := lifecycleReencode(ordinal, payload)
			if err != nil || !bytes.Equal(encoded, payload) {
				t.Fatalf("C wire differs: %v", err)
			}
			request, err := NewNativeProgramLifecycleRequest(ordinal, payload, signed)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(request.binding.ActivityID[:], lifecycleFixtureBytes(t, fixture, "activity_id_hex")) || !bytes.Equal(request.binding.IdempotencyKey[:], lifecycleFixtureBytes(t, fixture, "idempotency_key_hex")) {
				t.Fatal("signed request binding differs")
			}
			for length := 0; length < len(payload); length++ {
				if _, err := lifecycleReencode(ordinal, payload[:length]); err == nil {
					t.Fatalf("accepted prefix %d", length)
				}
			}
			if _, err := lifecycleReencode(ordinal, append(append([]byte{}, payload...), 0)); err == nil {
				t.Fatal("accepted trailing byte")
			}
			changedPayload := append([]byte{}, payload...)
			changedPayload[0] ^= 1
			if _, err := NewNativeProgramLifecycleRequest(ordinal, changedPayload, signed); err == nil {
				t.Fatal("accepted different payload")
			}
			for _, offset := range []int{1, 7, 17} {
				changed := append([]byte{}, signed...)
				changed[offset] ^= 1
				if _, err := NewNativeProgramLifecycleRequest(ordinal, payload, changed); err == nil {
					t.Fatalf("accepted changed protocol/type at %d", offset)
				}
			}
			for length := 0; length < len(signed); length++ {
				if _, err := NewNativeProgramLifecycleRequest(ordinal, payload, signed[:length]); err == nil {
					t.Fatalf("accepted signed prefix %d", length)
				}
			}
			if len(signed) < 69 || signed[len(signed)-69] != 12 || binary.BigEndian.Uint32(signed[len(signed)-68:]) != 64 {
				t.Fatal("C fixture signature framing")
			}
			unsigned := append([]byte{}, signed[:len(signed)-69]...)
			unsigned[4] = 11
			digest := domainDigest([]byte("LXP/v1/signature-preimage\x00"), unsigned)
			if !ed25519.Verify(lifecycleFixtureBytes(t, fixture, "public_key_hex"), digest[:], signed[len(signed)-64:]) {
				t.Fatal("C fixture signature rejected")
			}
		})
	}
}

func TestNativeLifecycleRefusals(t *testing.T) {
	for _, name := range []string{"deploy", "upgrade"} {
		ordinal, payload, _, _ := lifecycleFixture(t, name)
		for _, offset := range []int{35, 68, len(payload) - 1} {
			changed := append([]byte{}, payload...)
			changed[offset] ^= 1
			if _, err := lifecycleReencode(ordinal, changed); err == nil {
				t.Fatalf("accepted %s mutation %d", name, offset)
			}
		}
	}
	_, payload, _, _ := lifecycleFixture(t, "deploy")
	deploy, err := DecodeNativeProgramDeploy(payload)
	if err != nil {
		t.Fatal(err)
	}
	deploy.Interface = make([]byte, 953)
	if _, err := EncodeNativeProgramDeploy(deploy); err == nil {
		t.Fatal("oversized interface")
	}
	deploy.Interface = []byte{}
	if _, err := EncodeNativeProgramDeploy(deploy); err == nil {
		t.Fatal("empty deploy interface")
	}
	deploy.Interface = nil
	deploy.Policy = 0
	deploy.Authority[0] = 1
	if _, err := EncodeNativeProgramDeploy(deploy); err == nil {
		t.Fatal("immutable authority")
	}
	_, payload, _, _ = lifecycleFixture(t, "wind-down-route")
	route, err := DecodeNativeProgramWindDown(payload)
	if err != nil {
		t.Fatal(err)
	}
	route.Seed = make([]byte, 129)
	if _, err := EncodeNativeProgramWindDown(route); err == nil {
		t.Fatal("oversized seed")
	}
}

func TestNativeLifecycleHTTPRequestsAreSignedOctets(t *testing.T) {
	secret, err := NewSecretBytes([]byte("fixture-bearer"))
	if err != nil {
		t.Fatal(err)
	}
	defer secret.Destroy()
	transport, err := NewProgramBearerHTTPTransport("http://127.0.0.1:8080", nil, secret)
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"deploy", "upgrade", "wind-down-route", "wind-down-deprecate", "wind-down-tombstone", "wind-down-exit"} {
		ordinal, payload, signed, fixture := lifecycleFixture(t, name)
		operation := map[uint16]string{1: "program.deploy", 2: "program.upgrade", 7: "program.wind-down"}[ordinal]
		key, err := NewIdempotencyKey(hex.EncodeToString(lifecycleFixtureBytes(t, fixture, "idempotency_key_hex")))
		if err != nil {
			t.Fatal(err)
		}
		body, err := json.Marshal(map[string]string{"payload": hex.EncodeToString(payload), "signed_activity": hex.EncodeToString(signed)})
		if err != nil {
			t.Fatal(err)
		}
		call := TransportCall{Plane: PlaneAgent, Operation: operation, Request: body, IdempotencyKey: key}
		request, err := transport.request(context.Background(), call)
		if err != nil {
			t.Fatal(err)
		}
		encoded, err := io.ReadAll(request.Body)
		request.Body.Close()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(encoded, signed) || request.Method != "POST" || request.URL.Path != programLifecyclePaths[operation] || request.Header.Get("Content-Type") != "application/octet-stream" || request.Header.Get("Idempotency-Key") != key.String() || request.Header.Get("Authorization") != "Bearer fixture-bearer" {
			t.Fatal("lifecycle HTTP binding differs")
		}
		call.IdempotencyKey = IdempotencyKey{}
		if _, err := transport.request(context.Background(), call); err == nil {
			t.Fatal("missing idempotency accepted")
		}
		call.IdempotencyKey, err = NewIdempotencyKey(hex.EncodeToString(make([]byte, 32)))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := transport.request(context.Background(), call); err == nil {
			t.Fatal("different idempotency accepted")
		}
	}
}

func TestLifecycleVerifierRejectsCallOutcomeReceipt(t *testing.T) {
	raw, err := os.ReadFile("../conformance/fixtures/receipt-programs-positive-v3.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	canonical := lifecycleFixtureBytes(t, fixture, "canonical_receipt_hex")
	decoded, err := decodeProtocolReceipt(canonical)
	if err != nil {
		t.Fatal(err)
	}
	if decoded.protocol.Operation != 3 {
		t.Fatal("fixture is not a CALL outcome")
	}
	_, err = VerifyProgramLifecycleReceipt(canonical, decoded.activityID, [32]byte{1})
	refusal, ok := err.(*VerificationError)
	if !ok || refusal.Check != ReceiptCheckReceiptShape {
		t.Fatal("CALL outcome accepted as a lifecycle state receipt")
	}
}
