package layerx

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
)

const maximumHTTPResponseBytes = 8 * 1024 * 1024
const maximumHTTPProgramsRequestBytes = 8 * 1024 * 1024

type programHTTPRoute struct {
	method          string
	path            string
	pathParameters  []string
	idempotencyOnly bool
}

var programHTTPRoutes = map[string]programHTTPRoute{
	"program.discover":  {method: http.MethodGet, path: "/v1/programs/registry/{program_id}", pathParameters: []string{"program_id"}},
	"program.interface": {method: http.MethodGet, path: "/v1/programs/registry/{program_id}/interface", pathParameters: []string{"program_id"}},
	"program.simulate":  {method: http.MethodPost, path: "/v1/programs/simulate"},
	"program.call":      {method: http.MethodPost, path: "/v1/programs/call", idempotencyOnly: true},
	"program.receipt":   {method: http.MethodGet, path: "/v1/programs/receipts/by-idempotency/{idempotency_key}", pathParameters: []string{"idempotency_key"}},
	"program.activity":  {method: http.MethodGet, path: "/v1/programs/activities/{activity_id}", pathParameters: []string{"activity_id"}},
}

type RequestAuthorizer func(*http.Request) error

type HumanHTTPTransport struct {
	baseURL    *url.URL
	client     *http.Client
	authorizer RequestAuthorizer
}

func NewHumanHTTPTransport(baseURL string, client *http.Client, authorizer RequestAuthorizer) (*HumanHTTPTransport, error) {
	parsed, err := url.Parse(baseURL)
	if err != nil || parsed.Host == "" || parsed.User != nil || (parsed.Scheme != "https" && parsed.Scheme != "http") {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	if parsed.Scheme == "http" && !loopbackHost(parsed.Hostname()) {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	if client == nil {
		client = &http.Client{}
	}
	boundedClient := *client
	boundedClient.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &HumanHTTPTransport{baseURL: parsed, client: &boundedClient, authorizer: authorizer}, nil
}

func loopbackHost(host string) bool {
	if host == "localhost" {
		return true
	}
	address := net.ParseIP(host)
	return address != nil && address.IsLoopback()
}

func NewLayerXKeyAuthorizer(keyID string, secret string) (RequestAuthorizer, error) {
	if !validLayerXKeyID(keyID) || len(secret) != len("lxp_live_")+64 || !strings.HasPrefix(secret, "lxp_live_") || !canonicalLowerHex(secret[len("lxp_live_"):], 32) {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	authorization := "LayerX-Key " + keyID + ":" + secret
	return func(request *http.Request) error {
		if request == nil {
			return newSDKError(ErrorInvalidArgument, RetryNever)
		}
		request.Header.Set("Authorization", authorization)
		return nil
	}, nil
}

func validLayerXKeyID(value string) bool {
	if value == "" || len(value) > 64 {
		return false
	}
	for index := range value {
		byteValue := value[index]
		if !(byteValue >= 'a' && byteValue <= 'z' || byteValue >= 'A' && byteValue <= 'Z' || byteValue >= '0' && byteValue <= '9' || byteValue == '-' || byteValue == '_') {
			return false
		}
	}
	return true
}

func validLayerXAuthorization(value string) bool {
	credential := strings.TrimPrefix(value, "LayerX-Key ")
	if credential == value {
		return false
	}
	keyID, secret, ok := strings.Cut(credential, ":")
	return ok && validLayerXKeyID(keyID) && len(secret) == len("lxp_live_")+64 && strings.HasPrefix(secret, "lxp_live_") && canonicalLowerHex(secret[len("lxp_live_"):], 32)
}

func (transport *HumanHTTPTransport) Call(ctx context.Context, call TransportCall) (json.RawMessage, error) {
	if transport == nil {
		return nil, newSDKError(ErrorUnavailableCapability, RetryNever)
	}
	var method string
	var path string
	var bodyRequired bool
	var expectedParameters []string
	programRoute, isProgram := programHTTPRoutes[call.Operation]
	if call.Plane == PlaneHuman {
		operation := HumanOperation(call.Operation)
		metadata, ok := operation.Metadata()
		if !ok {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
		method = metadata.Method
		path = metadata.Path
		bodyRequired = metadata.Request != "Empty"
	} else if call.Plane == PlaneAgent && isProgram {
		method = programRoute.method
		path = programRoute.path
		bodyRequired = true
		expectedParameters = programRoute.pathParameters
		if len(call.Request) == 0 || len(call.Request) > maximumHTTPProgramsRequestBytes {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
		if programRoute.idempotencyOnly {
			if !canonicalProgramKey(call.IdempotencyKey) {
				return nil, newSDKError(ErrorIdempotencyRequired, RetryNever)
			}
		} else if call.IdempotencyKey.valid() {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
	} else {
		return nil, newSDKError(ErrorUnavailableCapability, RetryNever)
	}
	if isProgram && len(call.PathParameters) != len(expectedParameters) {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	for _, name := range expectedParameters {
		if call.PathParameters[name] == "" {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
	}
	for name, value := range call.PathParameters {
		if name == "" || value == "" {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
		path = strings.ReplaceAll(path, "{"+name+"}", url.PathEscape(value))
	}
	if strings.ContainsAny(path, "{}") {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	target := *transport.baseURL
	target.Path = strings.TrimRight(target.Path, "/") + path
	target.RawQuery = cloneValues(call.Query).Encode()

	var body io.Reader
	if bodyRequired {
		body = bytes.NewReader(call.Request)
	}
	request, err := http.NewRequestWithContext(ctx, method, target.String(), body)
	if err != nil {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("User-Agent", "layerx-go/0.1.0")
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	if isProgram && programRoute.idempotencyOnly {
		request.Header.Set("Idempotency-Key", call.IdempotencyKey.String())
	} else if !isProgram && call.IdempotencyKey.valid() {
		request.Header.Set("Idempotency-Key", call.IdempotencyKey.String())
	}
	if transport.authorizer != nil {
		if err := transport.authorizer(request); err != nil {
			return nil, transportError(ctx, err)
		}
	}
	if isProgram && transport.authorizer != nil && !validLayerXAuthorization(request.Header.Get("Authorization")) {
		return nil, newSDKError(ErrorCapabilityRefusal, RetryNever)
	}
	response, err := transport.client.Do(request)
	if err != nil {
		if isProgram && call.Operation == "program.call" {
			return nil, newSDKError(ErrorUnknownOutcome, RetryUnknownOutcome)
		}
		return nil, transportError(ctx, err)
	}
	defer response.Body.Close()
	limited := io.LimitReader(response.Body, maximumHTTPResponseBytes+1)
	encoded, err := io.ReadAll(limited)
	if err != nil {
		if isProgram && call.Operation == "program.call" {
			return nil, newSDKError(ErrorUnknownOutcome, RetryUnknownOutcome)
		}
		return nil, transportError(ctx, err)
	}
	if len(encoded) > maximumHTTPResponseBytes {
		if isProgram && call.Operation == "program.call" {
			return nil, newSDKError(ErrorUnknownOutcome, RetryUnknownOutcome)
		}
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if isProgram {
		value, decodeError := decodeProgramAgentEnvelope(response.StatusCode, encoded, call.Operation)
		if decodeError != nil && call.Operation == "program.call" && (decodeError.Code == ErrorDecodeFailure || decodeError.Code == ErrorVerificationFailure) {
			return nil, newSDKError(ErrorUnknownOutcome, RetryUnknownOutcome)
		}
		return value, decodeError
	}
	var envelope humanEnvelope
	if err := json.Unmarshal(encoded, &envelope); err != nil || envelope.Trace == "" {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if envelope.OK {
		if len(envelope.Result) == 0 || envelope.Error != nil || response.StatusCode < 200 || response.StatusCode >= 300 {
			return nil, newSDKError(ErrorDecodeFailure, RetryNever)
		}
		return append(json.RawMessage(nil), envelope.Result...), nil
	}
	if envelope.Error == nil || envelope.Error.Code == "" || response.StatusCode >= 200 && response.StatusCode < 300 {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	return nil, envelope.Error.sdkError(envelope.Trace)
}

func decodeProgramAgentEnvelope(status int, encoded []byte, operation string) (json.RawMessage, *SDKError) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(encoded, &fields); err != nil {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if _, failed := fields["class"]; failed {
		if status >= 200 && status < 300 || len(fields) != 5 {
			return nil, newSDKError(ErrorDecodeFailure, RetryNever)
		}
		return nil, decodeProgramAgentError(fields)
	}
	if status < 200 || status >= 300 || len(fields) != 3 {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	var requestID string
	if err := json.Unmarshal(fields["request_id"], &requestID); err != nil || requestID == "" || len(requestID) > 256 {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	value := fields["value"]
	if len(value) == 0 || bytes.Equal(value, []byte("null")) {
		return nil, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if !acceptedProgramVerification(operation, value, fields["verification_status"]) {
		return nil, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	return append(json.RawMessage(nil), value...), nil
}

func acceptedProgramVerification(operation string, value json.RawMessage, encoded json.RawMessage) bool {
	var verification map[string]json.RawMessage
	if decodeStrict(encoded, &verification) != nil || verification == nil {
		return false
	}
	if operation == "program.discover" || operation == "program.interface" {
		return exactProgramUnverified(verification, "server_side_receipt_verification_only")
	}
	var result map[string]json.RawMessage
	_ = decodeStrict(value, &result)
	var resultState string
	_ = json.Unmarshal(result["state"], &resultState)
	if (operation == "program.call" || operation == "program.receipt" || operation == "program.activity") && (resultState == "unknown" || resultState == "pending") {
		return exactProgramUnverified(verification, "receipt_pending")
	}
	if !exactFields(verification, "state", "level") {
		return false
	}
	var state string
	var level string
	return json.Unmarshal(verification["state"], &state) == nil && json.Unmarshal(verification["level"], &level) == nil && state == "Achieved" && level == "SequencerSigned"
}

func exactProgramUnverified(value map[string]json.RawMessage, reason string) bool {
	if !exactFields(value, "state", "requested", "achieved", "reason") {
		return false
	}
	var state string
	var requested string
	var achieved string
	var actualReason string
	return json.Unmarshal(value["state"], &state) == nil && json.Unmarshal(value["requested"], &requested) == nil && json.Unmarshal(value["achieved"], &achieved) == nil && json.Unmarshal(value["reason"], &actualReason) == nil && state == "Unverified" && requested == "SequencerSigned" && achieved == "Unverified" && actualReason == reason
}

func decodeProgramAgentError(fields map[string]json.RawMessage) *SDKError {
	var class AgentErrorClass
	var retriability AgentRetriability
	var requestID string
	var reason string
	if json.Unmarshal(fields["class"], &class) != nil || !class.Valid() || json.Unmarshal(fields["retriability"], &retriability) != nil || !retriability.Valid() || json.Unmarshal(fields["request_id"], &requestID) != nil || requestID == "" || len(requestID) > 256 || json.Unmarshal(fields["reason"], &reason) != nil || reason == "" || len(reason) > 256 {
		return newSDKError(ErrorDecodeFailure, RetryNever)
	}
	var resultCode *int32
	protocolResult := fields["protocol_result_code"]
	if len(protocolResult) == 0 {
		return newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if !bytes.Equal(protocolResult, []byte("null")) {
		var value int32
		if json.Unmarshal(protocolResult, &value) != nil {
			return newSDKError(ErrorDecodeFailure, RetryNever)
		}
		resultCode = &value
	}
	code := ErrorInternalFault
	switch class {
	case AgentErrorTransportFailure:
		code = ErrorTransportFailure
	case AgentErrorDeadline:
		code = ErrorDeadline
	case AgentErrorProtocolIncompatibility:
		code = ErrorProtocolIncompatible
	case AgentErrorUnavailableCapability:
		code = ErrorUnavailableCapability
	case AgentErrorCoreRejection:
		code = ErrorCoreRejection
	case AgentErrorVerificationFailure:
		code = ErrorVerificationFailure
	case AgentErrorPolicyRefusal:
		code = ErrorPolicyRefusal
	case AgentErrorCapabilityRefusal:
		code = ErrorCapabilityRefusal
	case AgentErrorBudgetRefusal:
		code = ErrorBudgetRefusal
	case AgentErrorRateLimit:
		code = ErrorRateLimit
	case AgentErrorIdempotencyConflict:
		code = ErrorIdempotencyConflict
	}
	retry := RetryNever
	if retriability == AgentRetryRetriable {
		retry = RetrySafe
	}
	result := newSDKError(code, retry)
	result.ServiceCode = string(class)
	result.RequestID = requestID
	result.ProtocolResultCode = resultCode
	return result
}

func canonicalLowerHex(value string, bytes int) bool {
	if len(value) != bytes*2 {
		return false
	}
	for index := range value {
		if !(value[index] >= '0' && value[index] <= '9' || value[index] >= 'a' && value[index] <= 'f') {
			return false
		}
	}
	return true
}

type humanEnvelope struct {
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result"`
	Error  *humanAPIError  `json:"error"`
	Trace  string          `json:"trace"`
}

type humanAPIError struct {
	Code             HumanErrorCode `json:"code"`
	Retry            string         `json:"retry"`
	RetryAfterMillis *uint64        `json:"retry_after_ms,omitempty"`
}

func (apiError *humanAPIError) sdkError(trace string) *SDKError {
	code := ErrorCoreRejection
	switch apiError.Code {
	case HumanErrorRateLimited:
		code = ErrorRateLimit
	case HumanErrorUnavailable, HumanErrorUpstreamDegraded:
		code = ErrorTransportFailure
	case HumanErrorRefusedByPolicy:
		code = ErrorPolicyRefusal
	case HumanErrorRefusedByBudget, HumanErrorRefusedByLimit:
		code = ErrorBudgetRefusal
	case HumanErrorRefusedByCapability, HumanErrorForbidden, HumanErrorUnauthenticated, HumanErrorSessionExpired, HumanErrorStepUpRequired:
		code = ErrorCapabilityRefusal
	case HumanErrorRefusedByProtocol:
		code = ErrorCoreRejection
	case HumanErrorConflict:
		code = ErrorIdempotencyConflict
	}
	retry := RetryNever
	switch apiError.Retry {
	case "retriable":
		retry = RetrySafe
	case "retriable-after":
		retry = RetryAfter
	case "structural", "final":
		retry = RetryNever
	default:
		return newSDKError(ErrorDecodeFailure, RetryNever)
	}
	result := newSDKError(code, retry)
	result.ServiceCode = string(apiError.Code)
	result.RequestID = trace
	result.RetryAfterMillis = apiError.RetryAfterMillis
	return result
}
