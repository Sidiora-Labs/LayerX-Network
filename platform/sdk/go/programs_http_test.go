package layerx

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"reflect"
	"strings"
	"testing"
)

func TestProgramHTTPRoutesMatchSchema(t *testing.T) {
	expected := map[string]programHTTPRoute{
		"program.discover":  {method: http.MethodGet, path: "/v1/programs/registry/{program_id}", pathParameters: []string{"program_id"}},
		"program.interface": {method: http.MethodGet, path: "/v1/programs/registry/{program_id}/interface", pathParameters: []string{"program_id"}},
		"program.simulate":  {method: http.MethodPost, path: "/v1/programs/simulate"},
		"program.call":      {method: http.MethodPost, path: "/v1/programs/call", idempotencyOnly: true},
		"program.receipt":   {method: http.MethodGet, path: "/v1/programs/receipts/by-idempotency/{idempotency_key}", pathParameters: []string{"idempotency_key"}},
		"program.activity":  {method: http.MethodGet, path: "/v1/programs/activities/{activity_id}", pathParameters: []string{"activity_id"}},
	}
	if !reflect.DeepEqual(programHTTPRoutes, expected) {
		t.Fatalf("Programs HTTP routes diverged from the schema: %#v", programHTTPRoutes)
	}
	for operation, route := range programHTTPRoutes {
		if route.idempotencyOnly != (operation == "program.call") {
			t.Fatalf("unexpected Programs idempotency route: %s", operation)
		}
	}
}

func TestLayerXKeyAuthorizerIsExactAndRedactsNoAlternateGrammar(t *testing.T) {
	secret := "lxp_live_" + strings.Repeat("a", 64)
	authorizer, err := NewLayerXKeyAuthorizer("beta_key-1", secret)
	if err != nil {
		t.Fatalf("construct LayerX-Key authorizer: %v", err)
	}
	request, err := http.NewRequest(http.MethodPost, "https://example.invalid/v1/programs/call", bytes.NewReader(nil))
	if err != nil {
		t.Fatalf("construct request: %v", err)
	}
	if err := authorizer(request); err != nil {
		t.Fatalf("authorize request: %v", err)
	}
	if authorization := request.Header.Get("Authorization"); authorization != "LayerX-Key beta_key-1:"+secret || !validLayerXAuthorization(authorization) {
		t.Fatalf("unexpected Programs authorization grammar")
	}
	for _, invalid := range []string{
		"Bearer " + secret,
		"LayerX-Key beta_key-1:lxp_live_" + strings.Repeat("A", 64),
		"LayerX-Key beta key:" + secret,
		"LayerX-Key beta_key-1:" + strings.TrimSuffix(secret, "a"),
	} {
		if validLayerXAuthorization(invalid) {
			t.Fatalf("accepted invalid Programs authorization: %q", invalid)
		}
	}
}

func TestProgramAgentEnvelopeRequiresExactAchievedSequencerProof(t *testing.T) {
	encoded := []byte(`{"request_id":"request-1","value":{"state":"unknown"},"verification_status":{"state":"Achieved","level":"SequencerSigned"}}`)
	if _, err := decodeProgramAgentEnvelope(http.StatusOK, encoded, "program.call"); err == nil || err.Code != ErrorVerificationFailure {
		t.Fatalf("pending Programs result accepted achieved verification: %v", err)
	}
	pending := []byte(`{"request_id":"request-1","value":{"state":"unknown"},"verification_status":{"state":"Unverified","requested":"SequencerSigned","achieved":"Unverified","reason":"receipt_pending"}}`)
	value, err := decodeProgramAgentEnvelope(http.StatusAccepted, pending, "program.call")
	if err != nil || string(value) != `{"state":"unknown"}` {
		t.Fatalf("decode exact pending Programs success: value=%s error=%v", value, err)
	}
	discovery := []byte(`{"request_id":"request-2","value":{"program_id":"` + strings.Repeat("a", 64) + `"},"verification_status":{"state":"Unverified","requested":"SequencerSigned","achieved":"Unverified","reason":"server_side_receipt_verification_only"}}`)
	if _, err := decodeProgramAgentEnvelope(http.StatusOK, discovery, "program.discover"); err != nil {
		t.Fatalf("server-attested discovery envelope was refused: %v", err)
	}
	terminal := []byte(`{"request_id":"request-3","value":{"state":"executed"},"verification_status":{"state":"Achieved","level":"SequencerSigned"}}`)
	if _, err := decodeProgramAgentEnvelope(http.StatusOK, terminal, "program.call"); err != nil {
		t.Fatalf("terminal Programs envelope was refused: %v", err)
	}
	for _, invalid := range [][]byte{
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Unverified","level":"SequencerSigned"}}`),
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Achieved","level":"StateProven"}}`),
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Achieved","level":"SequencerSigned"},"extra":true}`),
	} {
		if _, failure := decodeProgramAgentEnvelope(http.StatusOK, invalid, "program.simulate"); failure == nil || (failure.Code != ErrorDecodeFailure && failure.Code != ErrorVerificationFailure) {
			t.Fatalf("accepted invalid Programs success envelope: %s", invalid)
		}
	}
	errorEnvelope := []byte(`{"class":"CoreRejection","protocol_result_code":-7,"retriability":"Terminal","request_id":"request-2","reason":"core_refused"}`)
	if _, failure := decodeProgramAgentEnvelope(http.StatusBadRequest, errorEnvelope, "program.call"); failure == nil || failure.Code != ErrorCoreRejection || failure.Retry != RetryNever || failure.RequestID != "request-2" || failure.ProtocolResultCode == nil || *failure.ProtocolResultCode != -7 {
		t.Fatalf("decode exact Programs error envelope: %#v", failure)
	}
}

func TestProgramCallKeyAndUnknownSubmissionAreExact(t *testing.T) {
	keyText := strings.Repeat("a", 64)
	key, err := NewIdempotencyKey(keyText)
	if err != nil || !canonicalProgramKey(key) {
		t.Fatalf("canonical Programs key was refused: %v", err)
	}
	uppercase, err := NewIdempotencyKey(strings.Repeat("A", 64))
	if err != nil || canonicalProgramKey(uppercase) {
		t.Fatalf("uppercase Programs key was accepted")
	}
	activityText := strings.Repeat("b", 64)
	retained := []byte{0x01, 0x02, 0x03}
	raw := json.RawMessage(`{"state":"unknown","activity_id":"` + activityText + `","idempotency_key":"` + keyText + `","retained_signed_activity":"010203"}`)
	trusted := [32]byte{1}
	submission, decodeError := decodeProgramSubmission(raw, nil, nil, keyText, retained, trusted)
	if decodeError != nil || submission.State != ProgramSubmissionUnknown || submission.IdempotencyKey != keyText || !bytes.Equal(submission.RetainedSignedActivity, retained) {
		t.Fatalf("decode exact unknown Programs submission: %#v %v", submission, decodeError)
	}
	withExecution := json.RawMessage(`{"state":"unknown","activity_id":"` + activityText + `","idempotency_key":"` + keyText + `","retained_signed_activity":"010203","receipt":"00"}`)
	if _, decodeError := decodeProgramSubmission(withExecution, nil, nil, keyText, retained, trusted); decodeError == nil || decodeError.Code != ErrorDecodeFailure {
		t.Fatalf("unknown Programs submission accepted execution evidence")
	}
	resolution := json.RawMessage(`{"state":"unknown","activity_id":"` + activityText + `","idempotency_key":"` + keyText + `"}`)
	if _, decodeError := decodeProgramSubmission(resolution, nil, nil, keyText, nil, trusted); decodeError != nil {
		t.Fatalf("receipt/activity resolution refused unknown state without retained request: %v", decodeError)
	}
	if _, decodeError := decodeProgramSubmission(resolution, nil, nil, keyText, retained, trusted); decodeError == nil || decodeError.Code != ErrorVerificationFailure {
		t.Fatalf("submit accepted unknown state without retained signed activity")
	}
}

func TestProgramUsageRequiresNumericOutputValues(t *testing.T) {
	valid := json.RawMessage(`{"cpu_fuel":"1","memory_bytes":"2","storage_read_bytes":"3","storage_write_bytes":"4","output_values":5,"output_bytes":"6","fee_units":"7"}`)
	var usage ProgramUsage
	if decodeStrict(valid, &usage) != nil || usage.OutputValues != 5 || !validProgramUsage(usage) {
		t.Fatalf("numeric Programs output_values was refused: %#v", usage)
	}
	encodedString := bytes.Replace(valid, []byte(`"output_values":5`), []byte(`"output_values":"5"`), 1)
	if decodeStrict(encodedString, &usage) == nil {
		t.Fatalf("string Programs output_values was accepted")
	}
}

func TestProgramSourceTagsAreClosed(t *testing.T) {
	var unpublished ProgramSource
	if err := json.Unmarshal([]byte(`{"status":"unpublished"}`), &unpublished); err != nil || unpublished.Status != "unpublished" {
		t.Fatalf("decode unpublished source: %#v %v", unpublished, err)
	}
	verifiedJSON := `{"status":"verified","source_digest":"` + strings.Repeat("1", 64) + `","environment_digest":"` + strings.Repeat("2", 64) + `","pipeline":"sha256-source-artifact-reproducible-build-v1"}`
	var verified ProgramSource
	if err := json.Unmarshal([]byte(verifiedJSON), &verified); err != nil || verified.Status != "verified" || verified.SourceDigest == nil || verified.EnvironmentDigest == nil {
		t.Fatalf("decode verified source: %#v %v", verified, err)
	}
	for _, invalid := range []string{
		`{"status":"imagined"}`,
		`{"status":"unpublished","source_digest":"` + strings.Repeat("1", 64) + `"}`,
		strings.Replace(verifiedJSON, "sha256-source-artifact-reproducible-build-v1", "untrusted-pipeline", 1),
	} {
		var source ProgramSource
		if json.Unmarshal([]byte(invalid), &source) == nil {
			t.Fatalf("accepted invalid Programs source: %s", invalid)
		}
	}
}

func TestPendingProgramSubmissionNormalizesToUnknown(t *testing.T) {
	activity := [32]byte{1}
	key := strings.Repeat("02", 32)
	raw := json.RawMessage(`{"state":"pending","activity_id":"` + hex.EncodeToString(activity[:]) + `","idempotency_key":"` + key + `"}`)
	submission, err := decodeProgramSubmission(raw, nil, &activity, key, nil, [32]byte{3})
	if err != nil || submission.State != ProgramSubmissionUnknown || submission.ActivityID != activity || submission.IdempotencyKey != key {
		t.Fatalf("pending Programs submission was not retained as unknown: %#v %v", submission, err)
	}
}

func TestSignedProgramCallBindingDerivesCanonicalIdentityAndKey(t *testing.T) {
	call, key, activity := canonicalProgramCallFixture()
	binding, err := bindSignedProgramCall(call)
	if err != nil {
		t.Fatalf("bind canonical signed Programs call: %v", err)
	}
	if binding.IdempotencyKey != key || binding.ActivityID != domainDigest([]byte("LXP/v1/activity-id\x00"), activity) {
		t.Fatalf("signed Programs identity binding diverged")
	}
	mutated := call
	mutated.Calldata = []byte{0xaa, 0xbc}
	if _, err := bindSignedProgramCall(mutated); err == nil {
		t.Fatalf("signed Programs call accepted a different typed payload")
	}
	corrupted := call
	corrupted.SignedActivity = append([]byte(nil), call.SignedActivity...)
	corrupted.SignedActivity[len(corrupted.SignedActivity)-1] ^= 1
	corruptedBinding, err := bindSignedProgramCall(corrupted)
	if err != nil || corruptedBinding.ActivityID == binding.ActivityID {
		t.Fatalf("canonical signed bytes did not exclusively determine activity identity")
	}
}

func TestCanonicalProgramTerminalBindsResponseUsageAndGraph(t *testing.T) {
	program := [32]byte{1}
	graph := []byte{0x10, 0x20}
	terminal := []byte("LXP/program-execution/v4\x00")
	terminal = appendUint16(terminal, 1)
	terminal = appendUint32(terminal, 1)
	terminal = appendUint32(terminal, 1)
	terminal = appendUint64(terminal, 0)
	for _, value := range []uint64{1, 2, 3, 4} {
		terminal = appendUint64(terminal, value)
	}
	terminal = appendUint32(terminal, 5)
	terminal = appendUint64(terminal, 6)
	terminal = append(terminal, make([]byte, 15)...)
	terminal = append(terminal, 7)
	terminal = append(terminal, 0)
	terminal = append(terminal, program[:]...)
	terminal = appendUint16(terminal, 2)
	terminal = append(terminal, 0)
	terminal = appendUint32(terminal, 0)
	terminal = appendUint64(terminal, 2)
	terminal = append(terminal, 0xaa, 0xbb)
	terminal = appendUint64(terminal, uint64(len(graph)))
	terminal = append(terminal, graph...)
	projection, err := decodeProgramTerminal(1, 2, terminal, program, 0)
	if err != nil || !projection.Candidate || !projection.Successful || projection.Outcome.Kind != "completed" || !bytes.Equal(projection.Outcome.Response, []byte{0xaa, 0xbb}) || projection.OutputValues != 5 || projection.OutputBytes != 6 || projection.FeeUnits != NewUint128(0, 7) || !bytes.Equal(projection.EmbeddedGraph, graph) {
		t.Fatalf("canonical Programs terminal projection diverged: %#v %v", projection, err)
	}
	if _, err := decodeProgramTerminal(2, 2, terminal, program, -1); err == nil {
		t.Fatalf("Programs success terminal accepted failure receipt kind")
	}
}

func TestProgramTransferAuthorizationRecomputesCanonicalRoot(t *testing.T) {
	authorization, root := canonicalProgramTransferAuthorizationFixture()
	if err := verifyProgramTransferAuthorization(authorization, root); err != nil {
		t.Fatalf("canonical Programs transfer authorization rejected: %v", err)
	}
	mutated := append([]byte(nil), authorization...)
	mutated[len(mutated)-33] ^= 1
	if err := verifyProgramTransferAuthorization(mutated, root); err == nil {
		t.Fatalf("mutated Programs transfer authorization retained the signed root")
	}
	if err := verifyProgramTransferAuthorization(append(authorization, 0), root); err == nil {
		t.Fatalf("Programs transfer authorization accepted trailing bytes")
	}
}

func TestProgramOccupancySettlementBindsCountersFeesAndAssetRoot(t *testing.T) {
	asset := [32]byte{4}
	settlement := canonicalEmptyProgramOccupancyFixture()
	projection, err := decodeProgramOccupancy(settlement, asset)
	if err != nil || projection.ByteBatches != (Uint128{}) || projection.FeeUnits != (Uint128{}) || projection.TransferRoot != ([32]byte{}) {
		t.Fatalf("canonical empty Programs occupancy settlement rejected: %#v %v", projection, err)
	}
	mutated := append([]byte(nil), settlement...)
	declaredFeeLowByte := len("LXP/storage-occupancy-settlement/v3\x00") + 8 + 4 + 7*8 + 16 + 15
	mutated[declaredFeeLowByte] = 1
	if _, err := decodeProgramOccupancy(mutated, asset); err == nil {
		t.Fatalf("Programs occupancy settlement accepted a mutated fee total")
	}
	if _, err := decodeProgramOccupancy(settlement, [32]byte{}); err == nil {
		t.Fatalf("Programs occupancy settlement accepted a missing receipt asset")
	}
}

func TestProgramTerminalWrapperOrderAndUniquenessAreClosed(t *testing.T) {
	program := [32]byte{1}
	inner := []byte("LXP/program-execution-with-transfer-authority/v2\x00")
	inner = appendUint32(inner, 0)
	inner = appendUint32(inner, 0)
	inner = append(inner, make([]byte, 32)...)
	outer := []byte("LXP/program-execution-with-occupancy/v1\x00")
	outer = appendUint32(outer, uint32(len(inner)))
	outer = append(outer, inner...)
	outer = appendUint32(outer, 0)
	if _, err := decodeProgramTerminal(1, 2, outer, program, 0); err == nil {
		t.Fatalf("Programs terminal accepted producer-impossible occupancy-before-authority wrappers")
	}
}

func canonicalProgramTransferAuthorizationFixture() ([]byte, [32]byte) {
	program, principal, invocation, asset, destination := [32]byte{1}, [32]byte{2}, [32]byte{3}, [32]byte{4}, [32]byte{5}
	encoded := []byte("LayerX/programs/402LXP/transfer-set/v1\x00")
	encoded = append(encoded, program[:]...)
	encoded = append(encoded, principal[:]...)
	encoded = append(encoded, invocation[:]...)
	encoded = append(encoded, make([]byte, 8)...)
	encoded = append(encoded, 0)
	events := []byte("LayerX/programs/events/v1\x00")
	events = appendUint32(events, 0)
	encoded = appendUint32(encoded, uint32(len(events)))
	encoded = append(encoded, events...)
	encoded = appendUint64(encoded, 0)
	encoded = appendUint64(encoded, 1)
	encoded = append(encoded, make([]byte, 8)...)
	encoded = append(encoded, 0)
	encoded = append(encoded, asset[:]...)
	encoded = append(encoded, destination[:]...)
	encoded = append(encoded, make([]byte, 15)...)
	encoded = append(encoded, 7)
	encoded = append(encoded, program[:]...)
	leg := []byte{0}
	leg = append(leg, principal[:]...)
	leg = append(leg, destination[:]...)
	leg = append(leg, asset[:]...)
	leg = append(leg, make([]byte, 15)...)
	leg = append(leg, 7, 0, 1)
	return encoded, domainDigest([]byte("LXP/v1/merkle-leaf\x00"), leg)
}

func canonicalEmptyProgramOccupancyFixture() []byte {
	encoded := []byte("LXP/storage-occupancy-settlement/v3\x00")
	encoded = appendUint64(encoded, 1)
	encoded = appendUint32(encoded, 1)
	for index := 0; index < 7; index++ {
		encoded = appendUint64(encoded, uint64(index+1))
	}
	encoded = append(encoded, make([]byte, 16*4)...)
	return appendUint32(encoded, 0)
}

func canonicalProgramCallFixture() (ProgramCall, [32]byte, []byte) {
	program := [32]byte{1}
	key := [32]byte{2}
	calldata := []byte{0xaa, 0xbb}
	payload := []byte("LayerX/programs/call/v1\x00")
	payload = append(payload, program[:]...)
	payload = appendUint64(payload, 10)
	payload = append(payload, make([]byte, 15)...)
	payload = append(payload, 20)
	payload = appendUint16(payload, 1)
	payload = append(payload, 1)
	payload = appendUint32(payload, uint32(len(calldata)))
	payload = append(payload, calldata...)
	payloadHash := domainDigest([]byte("LXP/v1/payload-hash\x00"), payload)
	activity := appendUint16(nil, 1)
	activity = appendUint16(activity, 0x1001)
	activity = append(activity, 12, 1)
	activity = appendUint16(activity, 1)
	activity = append(activity, 2)
	activity = appendUint32(activity, 1)
	activity = append(activity, 3)
	activity = appendUint32(activity, 0x0009_0003)
	activity = append(activity, 4)
	activity = appendUint32(activity, 3)
	activity = append(activity, 'd', 'i', 'd')
	activity = append(activity, 5)
	activity = appendUint32(activity, 1)
	activity = append(activity, 1)
	activity = append(activity, 6)
	activity = appendUint64(activity, 1)
	activity = append(activity, 7)
	activity = appendUint64(activity, 1)
	activity = appendUint64(activity, 2)
	activity = append(activity, 8)
	activity = appendUint32(activity, 32)
	activity = append(activity, key[:]...)
	activity = append(activity, 9)
	activity = append(activity, make([]byte, 15)...)
	activity = append(activity, 30)
	activity = append(activity, 10)
	activity = appendUint32(activity, 32)
	activity = append(activity, payloadHash[:]...)
	activity = append(activity, 11)
	activity = appendUint32(activity, uint32(len(payload)))
	activity = append(activity, payload...)
	activity = append(activity, 12)
	activity = appendUint32(activity, 64)
	activity = append(activity, make([]byte, 64)...)
	return ProgramCall{ProgramID: program, Calldata: calldata, Budget: ProgramBudget{Fuel: 10, FeeLimit: NewUint128(0, 20)}, Capabilities: []ProgramCapability{ProgramStorageRead}, SignedActivity: activity}, key, activity
}
