package layerx

import (
	"bytes"
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
	value, err := decodeProgramAgentEnvelope(http.StatusOK, encoded)
	if err != nil || string(value) != `{"state":"unknown"}` {
		t.Fatalf("decode exact Programs success: value=%s error=%v", value, err)
	}
	for _, invalid := range [][]byte{
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Unverified","level":"SequencerSigned"}}`),
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Achieved","level":"StateProven"}}`),
		[]byte(`{"request_id":"request-1","value":{},"verification_status":{"state":"Achieved","level":"SequencerSigned"},"extra":true}`),
	} {
		if _, failure := decodeProgramAgentEnvelope(http.StatusOK, invalid); failure == nil || failure.Code != ErrorDecodeFailure {
			t.Fatalf("accepted invalid Programs success envelope: %s", invalid)
		}
	}
	errorEnvelope := []byte(`{"class":"CoreRejection","protocol_result_code":-7,"retriability":"Terminal","request_id":"request-2","reason":"core_refused"}`)
	if _, failure := decodeProgramAgentEnvelope(http.StatusBadRequest, errorEnvelope); failure == nil || failure.Code != ErrorCoreRejection || failure.Retry != RetryNever || failure.RequestID != "request-2" || failure.ProtocolResultCode == nil || *failure.ProtocolResultCode != -7 {
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
