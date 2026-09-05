package layerx

import (
	"encoding/json"
	"os"
	"testing"
)

func TestNativeProgramSignedBinding(t *testing.T) {
	raw, err := os.ReadFile("../conformance/fixtures/native-program-call-v3.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture map[string]json.RawMessage
	if err = json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	field := func(name string) []byte {
		var value string
		if err := json.Unmarshal(fixture[name], &value); err != nil {
			t.Fatal(err)
		}
		return fixtureBytes(t, value)
	}
	payload := field("payload_hex")
	native, err := DecodeNativeProgramCall(payload)
	if err != nil {
		t.Fatal(err)
	}
	request := NativeProgramRequest{Call: native, FeeLimit: NewUint128(0, 1000), SignedActivity: field("signed_activity_hex")}
	call, err := request.ProgramCall()
	if err != nil {
		t.Fatal(err)
	}
	binding, err := bindSignedProgramCall(call)
	if err != nil {
		t.Fatal(err)
	}
	if string(binding.ActivityID[:]) != string(field("activity_id_hex")) {
		t.Fatal("activity mismatch")
	}
	request.FeeLimit = NewUint128(0, 999)
	if _, err = request.ProgramCall(); err == nil {
		t.Fatal("fee mismatch accepted")
	}
	request.FeeLimit = NewUint128(0, 1000)
	request.Call.ResponseCapacity++
	if _, err = request.ProgramCall(); err == nil {
		t.Fatal("native payload mismatch accepted")
	}
	for n := 0; n < len(payload); n++ {
		if _, err := DecodeNativeProgramCall(payload[:n]); err == nil {
			t.Fatalf("truncated %d accepted", n)
		}
	}
}
