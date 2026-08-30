package layerx

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"math"
	"math/big"
	"strconv"
	"time"
)

const (
	MaximumProgramCalldataBytes = 1_048_576
	MaximumProgramCapabilities = 5
	maximumProgramLegacyValues = 512
)

type ProgramCapability string

const (
	ProgramStorageRead  ProgramCapability = "storage_read"
	ProgramStorageWrite ProgramCapability = "storage_write"
	ProgramTransfer     ProgramCapability = "transfer"
	ProgramEmitEvent    ProgramCapability = "emit_event"
	ProgramCompose      ProgramCapability = "compose"
)

type ProgramBudget struct {
	Fuel     uint64  `json:"fuel"`
	FeeLimit Uint128 `json:"fee_limit"`
}

type ProgramCall struct {
	ProgramID      [32]byte
	Calldata       []byte
	Budget         ProgramBudget
	Capabilities   []ProgramCapability
	SignedActivity []byte
}

type ProgramDiscovery struct {
	ProgramID        string `json:"program_id"`
	Lifecycle        string `json:"lifecycle"`
	Version          uint32 `json:"version"`
	CodeHash         string `json:"code_hash"`
	ABIVersion       uint16 `json:"abi_version"`
	ReceiptDigest    string `json:"receipt_digest"`
	StateRoot        string `json:"state_root"`
	ObservedSequence string `json:"observed_sequence"`
	ObservedAt       string `json:"observed_at"`
	ValidThrough     string `json:"valid_through"`
	Verification     string `json:"verification"`
}

type ProgramInterface struct {
	ProgramID        string          `json:"program_id"`
	Version          uint32          `json:"version"`
	CodeHash         string          `json:"code_hash"`
	ABIVersion       uint16          `json:"abi_version"`
	Interface        *string         `json:"interface"`
	InterfaceDigest  *string         `json:"interface_digest"`
	ReceiptDigest    string          `json:"receipt_digest"`
	StateRoot        string          `json:"state_root"`
	ObservedSequence string          `json:"observed_sequence"`
	ObservedAt       string          `json:"observed_at"`
	ValidThrough     string          `json:"valid_through"`
	Source           ProgramSource   `json:"source"`
	Verification     string          `json:"verification"`
}

type ProgramSource struct {
	Status                   string
	SourceDigest             *[32]byte
	EnvironmentDigest        *[32]byte
	ExpectedCodeHash         *[32]byte
	ReproducedArtifactDigest *[32]byte
	Pipeline                 string
}

func (source *ProgramSource) UnmarshalJSON(encoded []byte) error {
	fields, err := exactObject(encoded)
	if err != nil {
		return err
	}
	var status string
	if json.Unmarshal(fields["status"], &status) != nil {
		return errors.New("invalid Programs source status")
	}
	result := ProgramSource{Status: status}
	switch status {
	case "unpublished":
		if !exactFields(fields, "status") {
			return errors.New("invalid unpublished Programs source")
		}
	case "verified":
		if !exactFields(fields, "status", "source_digest", "environment_digest", "pipeline") {
			return errors.New("invalid verified Programs source")
		}
		sourceDigest, sourceError := programHex32Raw(fields["source_digest"])
		environmentDigest, environmentError := programHex32Raw(fields["environment_digest"])
		var pipeline string
		if sourceError != nil || environmentError != nil || json.Unmarshal(fields["pipeline"], &pipeline) != nil || pipeline != "sha256-source-artifact-reproducible-build-v1" {
			return errors.New("invalid verified Programs source")
		}
		result.SourceDigest = &sourceDigest
		result.EnvironmentDigest = &environmentDigest
		result.Pipeline = pipeline
	case "mismatch":
		if !exactFields(fields, "status", "expected_code_hash", "reproduced_artifact_digest") {
			return errors.New("invalid mismatched Programs source")
		}
		expected, expectedError := programHex32Raw(fields["expected_code_hash"])
		reproduced, reproducedError := programHex32Raw(fields["reproduced_artifact_digest"])
		if expectedError != nil || reproducedError != nil {
			return errors.New("invalid mismatched Programs source")
		}
		result.ExpectedCodeHash = &expected
		result.ReproducedArtifactDigest = &reproduced
	default:
		return errors.New("unknown Programs source status")
	}
	*source = result
	return nil
}

type ProgramResponseAuthority struct {
	BatchID            string `json:"batch_id"`
	Asset              string `json:"asset"`
	PreviousStateRoot  string `json:"previous_state_root"`
	ResultingStateRoot string `json:"resulting_state_root"`
	SequencerPublicKey string `json:"sequencer_public_key"`
}

type ProgramUsage struct {
	CPUFuel           string `json:"cpu_fuel"`
	MemoryBytes       string `json:"memory_bytes"`
	StorageReadBytes  string `json:"storage_read_bytes"`
	StorageWriteBytes string `json:"storage_write_bytes"`
	OutputValues      uint32 `json:"output_values"`
	OutputBytes       string `json:"output_bytes"`
	FeeUnits          string `json:"fee_units"`
}

type ProgramLegacyValue struct {
	Type  string `json:"type"`
	Value any    `json:"value"`
}

type ProgramFailure struct {
	Kind      string  `json:"kind"`
	Limit     *uint32 `json:"limit,omitempty"`
	Attempted *uint32 `json:"attempted,omitempty"`
	Code      *int32  `json:"code,omitempty"`
}

type ProgramOutcome struct {
	Kind     string
	Code     *int32
	Response []byte
	Values   []ProgramLegacyValue
	Failure  *ProgramFailure
}

func (outcome *ProgramOutcome) UnmarshalJSON(encoded []byte) error {
	fields, err := exactObject(encoded)
	if err != nil {
		return err
	}
	var kind string
	if json.Unmarshal(fields["kind"], &kind) != nil {
		return errors.New("invalid Programs outcome tag")
	}
	switch kind {
	case "completed":
		if !exactFields(fields, "kind", "code", "response") {
			return errors.New("invalid completed Programs outcome")
		}
		var code int32
		var response string
		if json.Unmarshal(fields["code"], &code) != nil || json.Unmarshal(fields["response"], &response) != nil || !canonicalLowerHex(response, len(response)/2) || len(response)/2 > MaximumProgramCalldataBytes {
			return errors.New("invalid completed Programs outcome")
		}
		body, _ := hex.DecodeString(response)
		*outcome = ProgramOutcome{Kind: kind, Code: &code, Response: body}
	case "legacy_completed":
		if !exactFields(fields, "kind", "code", "values") {
			return errors.New("invalid legacy Programs outcome")
		}
		var code int32
		var values []ProgramLegacyValue
		if json.Unmarshal(fields["code"], &code) != nil || decodeStrict(fields["values"], &values) != nil || len(values) > maximumProgramLegacyValues {
			return errors.New("invalid legacy Programs outcome")
		}
		for _, value := range values {
			if !validLegacyValue(value) {
				return errors.New("invalid legacy Programs value")
			}
		}
		*outcome = ProgramOutcome{Kind: kind, Code: &code, Values: values}
	case "refused":
		if !exactFields(fields, "kind", "failure") {
			return errors.New("invalid refused Programs outcome")
		}
		failure, decodeError := decodeProgramFailure(fields["failure"])
		if decodeError != nil {
			return decodeError
		}
		*outcome = ProgramOutcome{Kind: kind, Failure: &failure}
	default:
		return errors.New("unknown Programs outcome tag")
	}
	return nil
}

type ProgramExecutionDocument struct {
	State           string                   `json:"state"`
	ActivityID      string                   `json:"activity_id"`
	ProgramID       string                   `json:"program_id"`
	GuestABIVersion uint16                   `json:"guest_abi_version"`
	ModuleVersion   uint32                   `json:"module_version"`
	BatchID         string                   `json:"batch_id"`
	GlobalSequence  string                   `json:"global_sequence"`
	ResultCode      int32                    `json:"result_code"`
	StateRoot       string                   `json:"state_root"`
	Receipt         string                   `json:"receipt"`
	ReceiptDigest   string                   `json:"receipt_digest"`
	TerminalPayload string                   `json:"terminal_payload"`
	CallGraph       string                   `json:"call_graph"`
	Usage           ProgramUsage             `json:"usage"`
	Outcome         ProgramOutcome           `json:"outcome"`
	Verification    string                   `json:"verification"`
	Authority       ProgramResponseAuthority `json:"authority"`
	IdempotencyKey  string                   `json:"idempotency_key,omitempty"`
}

type ProgramSimulationEvidence struct {
	BoundaryID            string `json:"boundary_id"`
	ActivityID            string `json:"activity_id"`
	PreviousStateRoot     string `json:"previous_state_root"`
	HypotheticalStateRoot string `json:"hypothetical_state_root"`
	ObservedSequence      string `json:"observed_sequence"`
	ObservedAt            string `json:"observed_at"`
	Committed             bool   `json:"committed"`
	PublicKey             string `json:"public_key"`
	Signature             string `json:"signature"`
}

type ProgramSimulation struct {
	Committed          bool
	Execution          ProgramExecutionDocument
	SimulationEvidence ProgramSimulationEvidence
	Verification       VerifiedProgramReceipt
}

type ProgramSubmissionState string

const (
	ProgramSubmissionRefused  ProgramSubmissionState = "refused"
	ProgramSubmissionUnknown  ProgramSubmissionState = "unknown"
	ProgramSubmissionExecuted ProgramSubmissionState = "executed"
)

type ProgramSubmission struct {
	State                  ProgramSubmissionState
	ActivityID             [32]byte
	IdempotencyKey         string
	RetainedSignedActivity []byte
	Execution              *ProgramExecutionDocument
	Verification           *VerifiedProgramReceipt
}

type VerifiedProgramReceipt struct {
	Verification    VerifiedReceipt
	TerminalPayload []byte
	CallGraph       []byte
}

func validateProgramCall(call ProgramCall) error {
	if call.Budget.Fuel == 0 || len(call.Calldata) > MaximumProgramCalldataBytes || len(call.Capabilities) > MaximumProgramCapabilities || len(call.SignedActivity) == 0 || len(call.SignedActivity) > MaximumProgramCalldataBytes {
		return newSDKError(ErrorInvalidArgument, RetryNever)
	}
	order := map[ProgramCapability]int{ProgramStorageRead: 1, ProgramStorageWrite: 2, ProgramTransfer: 3, ProgramEmitEvent: 4, ProgramCompose: 5}
	prior := 0
	for _, capability := range call.Capabilities {
		current, ok := order[capability]
		if !ok || current <= prior {
			return newSDKError(ErrorInvalidArgument, RetryNever)
		}
		prior = current
	}
	return nil
}

func VerifyProgramReceipt(execution ProgramExecutionDocument, authority AuthorizedBatch) (VerifiedProgramReceipt, error) {
	if execution.ActivityID == "" || execution.ModuleVersion < 1 || execution.ModuleVersion > 3 || execution.GuestABIVersion != 1 && execution.GuestABIVersion != 2 {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	activity, err := programHex32(execution.ActivityID)
	if err != nil {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	receipt, err := decodeProgramHex(execution.Receipt)
	if err != nil {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	terminal, err := decodeProgramHex(execution.TerminalPayload)
	if err != nil {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	graph, err := decodeProgramHex(execution.CallGraph)
	if err != nil {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	verified, err := VerifyReceiptOutcome(receipt, authority)
	if err != nil {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	outcome := verified.Receipt.ProgramOutcome
	terminalDigest := sha256.Sum256(terminal)
	graphDigest := sha256.Sum256(graph)
	declaredReceiptDigest, digestError := programHex32(execution.ReceiptDigest)
	if digestError != nil || verified.Receipt.ActivityID != activity || verified.Receipt.BatchID != authority.BatchID || verified.Receipt.ResultingStateRoot != authority.ResultingStateRoot || verified.Receipt.ModuleID != 9 || verified.Receipt.Operation != 3 || verified.Receipt.ModuleVersion != execution.ModuleVersion || outcome == nil || outcome.ABIVersion != execution.GuestABIVersion || outcome.ResultCode != execution.ResultCode || len(graph) == 0 || terminalDigest != outcome.TerminalPayloadRoot || graphDigest != outcome.CallGraphRoot || verified.ReceiptDigest != declaredReceiptDigest {
		return VerifiedProgramReceipt{}, verificationFailure()
	}
	return VerifiedProgramReceipt{Verification: verified, TerminalPayload: terminal, CallGraph: graph}, nil
}

func decodeProgramHex(value string) ([]byte, error) {
	if len(value) > 2*MaximumProgramCalldataBytes || len(value)%2 != 0 || !canonicalLowerHex(value, len(value)/2) {
		return nil, errors.New("program evidence is not bounded canonical hexadecimal")
	}
	decoded, err := hex.DecodeString(value)
	if err != nil {
		return nil, errors.New("program evidence is not hexadecimal")
	}
	return decoded, nil
}

func programCallWire(call ProgramCall) map[string]any {
	return map[string]any{
		"program_id": hex.EncodeToString(call.ProgramID[:]),
		"calldata": hex.EncodeToString(call.Calldata),
		"budget": map[string]any{"fuel": programUint64(call.Budget.Fuel), "fee_limit": call.Budget.FeeLimit.String()},
		"capabilities": call.Capabilities,
		"signed_activity": hex.EncodeToString(call.SignedActivity),
	}
}

func programUint64(value uint64) string { return strconv.FormatUint(value, 10) }

func canonicalProgramKey(key IdempotencyKey) bool {
	return key.valid() && canonicalLowerHex(key.String(), 32)
}

type Programs struct {
	client                    *Client
	trustedSequencerPublicKey [32]byte
}

func NewPrograms(client *Client, trustedSequencerPublicKey [32]byte) (*Programs, error) {
	if client == nil || trustedSequencerPublicKey == ([32]byte{}) {
		return nil, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	return &Programs{client: client, trustedSequencerPublicKey: trustedSequencerPublicKey}, nil
}

func (programs *Programs) Discover(ctx context.Context, program [32]byte) (ProgramDiscovery, error) {
	id := hex.EncodeToString(program[:])
	raw, err := programs.raw(ctx, "program.discover", false, map[string]any{"program_id": id, "requested_verification_level": "sequencer-signed"}, CallOptions{PathParameters: map[string]string{"program_id": id}})
	if err != nil {
		return ProgramDiscovery{}, err
	}
	var out ProgramDiscovery
	if decodeStrict(raw, &out) != nil || !validDiscovery(out, id, uint64(time.Now().UnixMilli())) {
		return ProgramDiscovery{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	return out, nil
}

func (programs *Programs) Interface(ctx context.Context, program [32]byte) (ProgramInterface, error) {
	id := hex.EncodeToString(program[:])
	raw, err := programs.raw(ctx, "program.interface", false, map[string]any{"program_id": id, "requested_verification_level": "sequencer-signed"}, CallOptions{PathParameters: map[string]string{"program_id": id}})
	if err != nil {
		return ProgramInterface{}, err
	}
	var out ProgramInterface
	if decodeStrict(raw, &out) != nil || !validProgramInterface(out, id, uint64(time.Now().UnixMilli())) {
		return ProgramInterface{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	return out, nil
}

func (programs *Programs) Simulate(ctx context.Context, call ProgramCall) (ProgramSimulation, error) {
	if err := validateProgramCall(call); err != nil {
		return ProgramSimulation{}, err
	}
	raw, err := programs.raw(ctx, "program.simulate", false, programCallWire(call), CallOptions{})
	if err != nil {
		return ProgramSimulation{}, err
	}
	return decodeProgramSimulation(raw, call.ProgramID, programs.trustedSequencerPublicKey)
}

func (programs *Programs) Submit(ctx context.Context, call ProgramCall, key IdempotencyKey) (ProgramSubmission, error) {
	if err := validateProgramCall(call); err != nil {
		return ProgramSubmission{}, err
	}
	if !canonicalProgramKey(key) {
		return ProgramSubmission{}, newSDKError(ErrorIdempotencyRequired, RetryNever)
	}
	raw, err := programs.raw(ctx, "program.call", true, programCallWire(call), CallOptions{IdempotencyKey: key})
	if err != nil {
		return ProgramSubmission{}, err
	}
	return decodeProgramSubmission(raw, &call.ProgramID, nil, key.String(), call.SignedActivity, programs.trustedSequencerPublicKey)
}

func (programs *Programs) Receipt(ctx context.Context, key IdempotencyKey, expectedActivity [32]byte) (ProgramSubmission, error) {
	if !canonicalProgramKey(key) {
		return ProgramSubmission{}, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	activity := hex.EncodeToString(expectedActivity[:])
	raw, err := programs.raw(ctx, "program.receipt", false, map[string]any{"idempotency_key": key.String(), "expected_activity_id": activity, "requested_verification_level": "sequencer-signed"}, CallOptions{PathParameters: map[string]string{"idempotency_key": key.String()}})
	if err != nil {
		return ProgramSubmission{}, err
	}
	return decodeProgramSubmission(raw, nil, &expectedActivity, key.String(), nil, programs.trustedSequencerPublicKey)
}

func (programs *Programs) Activity(ctx context.Context, activity [32]byte) (ProgramSubmission, error) {
	id := hex.EncodeToString(activity[:])
	raw, err := programs.raw(ctx, "program.activity", false, map[string]any{"activity_id": id, "requested_verification_level": "sequencer-signed"}, CallOptions{PathParameters: map[string]string{"activity_id": id}})
	if err != nil {
		return ProgramSubmission{}, err
	}
	return decodeProgramSubmission(raw, nil, &activity, "", nil, programs.trustedSequencerPublicKey)
}

func (programs *Programs) raw(ctx context.Context, operation string, requiresKey bool, request any, options CallOptions) (json.RawMessage, error) {
	var out json.RawMessage
	err := programs.client.call(ctx, PlaneAgent, operation, requiresKey, request, &out, options)
	return out, err
}

func decodeProgramSimulation(raw json.RawMessage, expectedProgram [32]byte, trustedSequencerPublicKey [32]byte) (ProgramSimulation, error) {
	var document struct {
		Committed          bool                      `json:"committed"`
		Execution          ProgramExecutionDocument  `json:"execution"`
		SimulationEvidence ProgramSimulationEvidence `json:"simulation_evidence"`
	}
	if decodeStrict(raw, &document) != nil || document.Committed || document.Execution.State != "simulated" {
		return ProgramSimulation{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	programID, err := programHex32(document.Execution.ProgramID)
	if err != nil || programID != expectedProgram {
		return ProgramSimulation{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	verified, verifyError := verifyExecutionAuthority(document.Execution, trustedSequencerPublicKey)
	if verifyError != nil || verifySimulationEvidence(document.Execution, document.SimulationEvidence, trustedSequencerPublicKey) != nil {
		return ProgramSimulation{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	return ProgramSimulation{Committed: false, Execution: document.Execution, SimulationEvidence: document.SimulationEvidence, Verification: verified}, nil
}

func decodeProgramSubmission(raw json.RawMessage, expectedProgram *[32]byte, expectedActivity *[32]byte, expectedKey string, expectedSigned []byte, trustedSequencerPublicKey [32]byte) (ProgramSubmission, error) {
	fields, err := exactObject(raw)
	if err != nil {
		return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	var state ProgramSubmissionState
	if json.Unmarshal(fields["state"], &state) != nil {
		return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	if state == ProgramSubmissionUnknown {
		if !(exactFields(fields, "state", "activity_id", "idempotency_key") || exactFields(fields, "state", "activity_id", "idempotency_key", "retained_signed_activity")) {
			return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
		}
		var activityText, key string
		if json.Unmarshal(fields["activity_id"], &activityText) != nil || json.Unmarshal(fields["idempotency_key"], &key) != nil || !canonicalLowerHex(key, 32) || expectedKey != "" && key != expectedKey {
			return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
		}
		activity, activityError := programHex32(activityText)
		var retained []byte
		var retainedError error
		if retainedRaw := fields["retained_signed_activity"]; len(retainedRaw) != 0 {
			var retainedText string
			if json.Unmarshal(retainedRaw, &retainedText) != nil {
				return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
			}
			retained, retainedError = decodeProgramHex(retainedText)
			if retainedError == nil && len(retained) == 0 {
				retainedError = errors.New("empty retained signed activity")
			}
		}
		if activityError != nil || retainedError != nil || expectedActivity != nil && activity != *expectedActivity || expectedSigned != nil && !bytes.Equal(retained, expectedSigned) {
			return ProgramSubmission{}, newSDKError(ErrorVerificationFailure, RetryNever)
		}
		return ProgramSubmission{State: state, ActivityID: activity, IdempotencyKey: key, RetainedSignedActivity: retained}, nil
	}
	if state != ProgramSubmissionExecuted && state != ProgramSubmissionRefused {
		return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	var execution ProgramExecutionDocument
	if decodeStrict(raw, &execution) != nil || execution.State != string(state) || execution.IdempotencyKey == "" || !canonicalLowerHex(execution.IdempotencyKey, 32) || expectedKey != "" && execution.IdempotencyKey != expectedKey {
		return ProgramSubmission{}, newSDKError(ErrorDecodeFailure, RetryNever)
	}
	activity, activityError := programHex32(execution.ActivityID)
	programID, programError := programHex32(execution.ProgramID)
	if activityError != nil || programError != nil || expectedActivity != nil && activity != *expectedActivity || expectedProgram != nil && programID != *expectedProgram {
		return ProgramSubmission{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	if state == ProgramSubmissionRefused && execution.Outcome.Kind != "refused" || state == ProgramSubmissionExecuted && execution.Outcome.Kind != "completed" && execution.Outcome.Kind != "legacy_completed" {
		return ProgramSubmission{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	verified, verifyError := verifyExecutionAuthority(execution, trustedSequencerPublicKey)
	if verifyError != nil {
		return ProgramSubmission{}, newSDKError(ErrorVerificationFailure, RetryNever)
	}
	return ProgramSubmission{State: state, ActivityID: activity, IdempotencyKey: execution.IdempotencyKey, Execution: &execution, Verification: &verified}, nil
}

func verifyExecutionAuthority(execution ProgramExecutionDocument, trustedSequencerPublicKey [32]byte) (VerifiedProgramReceipt, error) {
	authority, err := execution.Authority.authorizedBatch()
	if err != nil || trustedSequencerPublicKey == ([32]byte{}) || authority.SequencerPublicKey != trustedSequencerPublicKey {
		return VerifiedProgramReceipt{}, err
	}
	stateRoot, rootError := programHex32(execution.StateRoot)
	batchID, batchError := programHex32(execution.BatchID)
	if rootError != nil || batchError != nil || stateRoot != authority.ResultingStateRoot || batchID != authority.BatchID || execution.Verification != "receipt-terminal-and-call-graph-verified" || !canonicalUnsigned(execution.GlobalSequence, 64) || !validProgramUsage(execution.Usage) {
		return VerifiedProgramReceipt{}, errors.New("invalid Programs execution authority")
	}
	verified, verifyError := VerifyProgramReceipt(execution, authority)
	if verifyError != nil {
		return VerifiedProgramReceipt{}, verifyError
	}
	receipt := verified.Verification.Receipt
	receiptOutcome := receipt.ProgramOutcome
	sequence, sequenceError := strconv.ParseUint(execution.GlobalSequence, 10, 64)
	feeUnits, feeError := ParseUint128(execution.Usage.FeeUnits)
	if receiptOutcome == nil || sequenceError != nil || receipt.GlobalSequence != sequence || feeError != nil || receipt.ResultCode != execution.ResultCode || receiptOutcome.CPUFuel != mustProgramUint64(execution.Usage.CPUFuel) || receiptOutcome.MemoryBytes != mustProgramUint64(execution.Usage.MemoryBytes) || receiptOutcome.StorageReadBytes != mustProgramUint64(execution.Usage.StorageReadBytes) || receiptOutcome.StorageWriteBytes != mustProgramUint64(execution.Usage.StorageWriteBytes) || receiptOutcome.OutputValues != execution.Usage.OutputValues || receiptOutcome.OutputBytes != mustProgramUint64(execution.Usage.OutputBytes) || !receiptOutcome.FeeUnits.Equal(feeUnits) || execution.Outcome.Kind == "completed" && (receiptOutcome.TerminalKind != 1 || execution.Outcome.Code == nil || *execution.Outcome.Code != receiptOutcome.ResultCode) || execution.Outcome.Kind == "legacy_completed" && (receiptOutcome.TerminalKind != 2 || execution.Outcome.Code == nil || *execution.Outcome.Code != receiptOutcome.ResultCode) || execution.Outcome.Kind == "refused" && receiptOutcome.TerminalKind != 3 {
		return VerifiedProgramReceipt{}, errors.New("Programs outcome is not receipt-bound")
	}
	return verified, nil
}

func verifySimulationEvidence(execution ProgramExecutionDocument, evidence ProgramSimulationEvidence, trustedSequencerPublicKey [32]byte) error {
	if evidence.Committed || !canonicalUnsigned(evidence.ObservedSequence, 64) || !canonicalUnsigned(evidence.ObservedAt, 64) {
		return errors.New("invalid simulation evidence")
	}
	boundary, boundaryError := programHex32(evidence.BoundaryID)
	activity, activityError := programHex32(evidence.ActivityID)
	previous, previousError := programHex32(evidence.PreviousStateRoot)
	hypothetical, hypotheticalError := programHex32(evidence.HypotheticalStateRoot)
	publicKey, publicError := programHex32(evidence.PublicKey)
	signature, signatureError := decodeProgramHex(evidence.Signature)
	executionActivity, executionActivityError := programHex32(execution.ActivityID)
	executionRoot, executionRootError := programHex32(execution.StateRoot)
	if boundaryError != nil || activityError != nil || previousError != nil || hypotheticalError != nil || publicError != nil || signatureError != nil || len(signature) != ed25519.SignatureSize || executionActivityError != nil || executionRootError != nil || activity != executionActivity || hypothetical != executionRoot {
		return errors.New("invalid simulation evidence")
	}
	authority, authorityError := execution.Authority.authorizedBatch()
	sequence, sequenceError := strconv.ParseUint(evidence.ObservedSequence, 10, 64)
	if authorityError != nil || sequenceError != nil || sequence == math.MaxUint64 || trustedSequencerPublicKey == ([32]byte{}) || authority.SequencerPublicKey != trustedSequencerPublicKey || publicKey != trustedSequencerPublicKey || execution.Verification != "receipt-terminal-and-call-graph-verified" || authority.PreviousStateRoot != previous || authority.ResultingStateRoot != hypothetical || execution.GlobalSequence != strconv.FormatUint(sequence+1, 10) {
		return errors.New("simulation authority mismatch")
	}
	expectedBoundary := sha256.Sum256(append([]byte("LayerX/emulator/simulation-boundary/v1\x00"), publicKey[:]...))
	if boundary != expectedBoundary {
		return errors.New("simulation boundary mismatch")
	}
	observedAt, _ := strconv.ParseUint(evidence.ObservedAt, 10, 64)
	signed := make([]byte, 0, 32*4+8*2+64)
	signed = append(signed, []byte("LayerX/agent/program-simulation-evidence/v1\x00")...)
	signed = append(signed, boundary[:]...)
	signed = append(signed, activity[:]...)
	signed = append(signed, previous[:]...)
	signed = append(signed, hypothetical[:]...)
	var integer [8]byte
	binary.BigEndian.PutUint64(integer[:], sequence)
	signed = append(signed, integer[:]...)
	binary.BigEndian.PutUint64(integer[:], observedAt)
	signed = append(signed, integer[:]...)
	signed = append(signed, 0)
	digest := sha256.Sum256(signed)
	if !ed25519.Verify(publicKey[:], digest[:], signature) {
		return errors.New("simulation signature mismatch")
	}
	return nil
}

func (authority ProgramResponseAuthority) authorizedBatch() (AuthorizedBatch, error) {
	batch, batchError := programHex32(authority.BatchID)
	asset, assetError := programHex32(authority.Asset)
	previous, previousError := programHex32(authority.PreviousStateRoot)
	resulting, resultingError := programHex32(authority.ResultingStateRoot)
	publicKey, keyError := programHex32(authority.SequencerPublicKey)
	if batchError != nil || assetError != nil || previousError != nil || resultingError != nil || keyError != nil {
		return AuthorizedBatch{}, errors.New("invalid response authority")
	}
	return AuthorizedBatch{BatchID: batch, Asset: asset, PreviousStateRoot: previous, ResultingStateRoot: resulting, SequencerPublicKey: publicKey}, nil
}

func validDiscovery(value ProgramDiscovery, expectedID string, now uint64) bool {
	return value.ProgramID == expectedID && canonicalLowerHex(value.CodeHash, 32) && canonicalLowerHex(value.ReceiptDigest, 32) && canonicalLowerHex(value.StateRoot, 32) && value.Version != 0 && value.ABIVersion >= 1 && value.ABIVersion <= 2 && (value.Lifecycle == "active" || value.Lifecycle == "deprecated" || value.Lifecycle == "tombstoned") && canonicalUnsigned(value.ObservedSequence, 64) && canonicalUnsigned(value.ObservedAt, 64) && canonicalUnsigned(value.ValidThrough, 64) && decimalAtLeast(value.ValidThrough, value.ObservedAt) && decimalAtLeast(value.ValidThrough, strconv.FormatUint(now, 10)) && value.Verification == "registry-receipt-and-current-head-verified"
}

func validProgramInterface(value ProgramInterface, expectedID string, now uint64) bool {
	if value.ProgramID != expectedID || !canonicalLowerHex(value.CodeHash, 32) || !canonicalLowerHex(value.ReceiptDigest, 32) || !canonicalLowerHex(value.StateRoot, 32) || value.Version == 0 || value.ABIVersion < 1 || value.ABIVersion > 2 || !canonicalUnsigned(value.ObservedSequence, 64) || !canonicalUnsigned(value.ObservedAt, 64) || !canonicalUnsigned(value.ValidThrough, 64) || !decimalAtLeast(value.ValidThrough, value.ObservedAt) || !decimalAtLeast(value.ValidThrough, strconv.FormatUint(now, 10)) || value.Verification != "deployment-interface-and-current-head-verified" || value.Source.Status == "" {
		return false
	}
	if value.Interface == nil || value.InterfaceDigest == nil {
		return false
	}
	decoded, err := decodeProgramHex(*value.Interface)
	digest, digestError := programHex32(*value.InterfaceDigest)
	return err == nil && len(decoded) != 0 && digestError == nil && sha256.Sum256(decoded) == digest
}

func validProgramUsage(usage ProgramUsage) bool {
	return canonicalUnsigned(usage.CPUFuel, 64) && canonicalUnsigned(usage.MemoryBytes, 64) && canonicalUnsigned(usage.StorageReadBytes, 64) && canonicalUnsigned(usage.StorageWriteBytes, 64) && canonicalUnsigned(usage.OutputBytes, 64) && canonicalUnsigned(usage.FeeUnits, 128)
}

func canonicalUnsigned(value string, bits int) bool {
	if value == "" || value != "0" && value[0] == '0' {
		return false
	}
	integer, ok := new(big.Int).SetString(value, 10)
	return ok && integer.Sign() >= 0 && integer.BitLen() <= bits
}

func decimalAtLeast(value string, minimum string) bool {
	parsed, parsedOK := new(big.Int).SetString(value, 10)
	lower, lowerOK := new(big.Int).SetString(minimum, 10)
	return parsedOK && lowerOK && parsed.Cmp(lower) >= 0
}

func mustProgramUint64(value string) uint64 {
	parsed, _ := strconv.ParseUint(value, 10, 64)
	return parsed
}

func validLegacyValue(value ProgramLegacyValue) bool {
	switch value.Type {
	case "i32":
		number, ok := value.Value.(float64)
		return ok && math.Trunc(number) == number && number >= math.MinInt32 && number <= math.MaxInt32
	case "i64":
		text, ok := value.Value.(string)
		if !ok || text == "" || text == "-0" || len(text) > 20 || text[0] == '0' && len(text) > 1 || text[0] == '-' && (len(text) == 1 || text[1] == '0' && len(text) > 2) {
			return false
		}
		_, err := strconv.ParseInt(text, 10, 64)
		return err == nil
	default:
		return false
	}
}

func decodeProgramFailure(raw json.RawMessage) (ProgramFailure, error) {
	fields, err := exactObject(raw)
	if err != nil {
		return ProgramFailure{}, err
	}
	var kind string
	if json.Unmarshal(fields["kind"], &kind) != nil {
		return ProgramFailure{}, errors.New("invalid Programs failure")
	}
	result := ProgramFailure{Kind: kind}
	switch kind {
	case "unknown_program", "reentrancy", "authority", "resource", "response", "fault":
		if !exactFields(fields, "kind") {
			return ProgramFailure{}, errors.New("invalid Programs failure")
		}
	case "depth_exceeded", "fanout_exceeded":
		if !exactFields(fields, "kind", "limit", "attempted") || json.Unmarshal(fields["limit"], &result.Limit) != nil || json.Unmarshal(fields["attempted"], &result.Attempted) != nil || result.Limit == nil || result.Attempted == nil {
			return ProgramFailure{}, errors.New("invalid Programs failure")
		}
	case "guest_refused":
		if !exactFields(fields, "kind", "code") || json.Unmarshal(fields["code"], &result.Code) != nil || result.Code == nil {
			return ProgramFailure{}, errors.New("invalid Programs failure")
		}
	default:
		return ProgramFailure{}, errors.New("unknown Programs failure")
	}
	return result, nil
}

func exactObject(encoded []byte) (map[string]json.RawMessage, error) {
	var fields map[string]json.RawMessage
	if decodeStrict(encoded, &fields) != nil || fields == nil {
		return nil, errors.New("invalid Programs object")
	}
	return fields, nil
}

func exactFields(fields map[string]json.RawMessage, required ...string) bool {
	if len(fields) != len(required) {
		return false
	}
	for _, name := range required {
		if len(fields[name]) == 0 {
			return false
		}
	}
	return true
}

func decodeStrict(encoded []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return errors.New("trailing JSON")
	}
	return nil
}

func programHex32(value string) ([32]byte, error) {
	if !canonicalLowerHex(value, 32) {
		return [32]byte{}, errors.New("invalid bytes32")
	}
	decoded, err := hex.DecodeString(value)
	if err != nil {
		return [32]byte{}, err
	}
	return [32]byte(decoded), nil
}

func programHex32Raw(value json.RawMessage) ([32]byte, error) {
	var text string
	if json.Unmarshal(value, &text) != nil {
		return [32]byte{}, errors.New("invalid bytes32")
	}
	return programHex32(text)
}

func PlatformSDKPrograms() string { return "receipt-verified-program-operations-v1" }
