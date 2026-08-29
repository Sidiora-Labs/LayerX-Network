package layerx

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"strconv"
)

const (
	MaximumProgramCalldataBytes = 1_048_576
	MaximumProgramCapabilities = 5
)

type ProgramCapability string

const (
	ProgramStorageRead  ProgramCapability = "storage_read"
	ProgramStorageWrite ProgramCapability = "storage_write"
	ProgramTransfer     ProgramCapability = "transfer"
	ProgramEmitEvent    ProgramCapability = "emit_event"
	ProgramCompose      ProgramCapability = "compose"
)

type ProgramBudget struct { Fuel uint64 `json:"fuel"`; FeeLimit Uint128 `json:"fee_limit"` }
type ProgramCall struct { ProgramID [32]byte; Calldata []byte; Budget ProgramBudget; Capabilities []ProgramCapability; SignedActivity []byte }
type ProgramDiscovery map[string]any
type ProgramInterface map[string]any
type ProgramExecutionDocument struct { State string `json:"state"`; ActivityID string `json:"activity_id"`; ProgramID string `json:"program_id"`; GuestABIVersion uint16 `json:"guest_abi_version"`; ModuleVersion uint32 `json:"module_version"`; ResultCode int32 `json:"result_code"`; StateRoot string `json:"state_root"`; Receipt string `json:"receipt"`; TerminalPayload string `json:"terminal_payload"`; CallGraph string `json:"call_graph"`; Outcome map[string]any `json:"outcome"`; IdempotencyKey string `json:"idempotency_key,omitempty"` }
type ProgramSimulation struct { Committed bool `json:"committed"`; Execution ProgramExecutionDocument `json:"execution"`; SimulationEvidence map[string]any `json:"simulation_evidence"` }
type ProgramSubmission struct { State string `json:"state"`; ActivityID string `json:"activity_id"`; IdempotencyKey string `json:"idempotency_key"`; RetainedSignedActivity string `json:"retained_signed_activity,omitempty"`; ProgramExecutionDocument }
type VerifiedProgramReceipt struct { Verification VerifiedReceipt; TerminalPayload []byte; CallGraph []byte }

func validateProgramCall(call ProgramCall) error {
	if call.Budget.Fuel == 0 || len(call.Calldata) > MaximumProgramCalldataBytes || len(call.Capabilities) > MaximumProgramCapabilities || len(call.SignedActivity) == 0 || len(call.SignedActivity) > MaximumProgramCalldataBytes { return errors.New("invalid program call") }
	order := map[ProgramCapability]int{ProgramStorageRead: 1, ProgramStorageWrite: 2, ProgramTransfer: 3, ProgramEmitEvent: 4, ProgramCompose: 5}
	prior := 0
	for _, capability := range call.Capabilities { current, ok := order[capability]; if !ok || current <= prior { return errors.New("invalid program capability") }; prior = current }
	return nil
}

func VerifyProgramReceipt(execution ProgramExecutionDocument, authority AuthorizedBatch) (VerifiedProgramReceipt, error) {
	if execution.ActivityID == "" || execution.ModuleVersion < 1 || execution.ModuleVersion > 3 || (execution.GuestABIVersion != 1 && execution.GuestABIVersion != 2) { return VerifiedProgramReceipt{}, errors.New("invalid program execution evidence") }
	activity, err := hex.DecodeString(execution.ActivityID); if err != nil || len(activity) != 32 { return VerifiedProgramReceipt{}, errors.New("invalid activity id") }
	receipt, err := decodeProgramHex(execution.Receipt); if err != nil { return VerifiedProgramReceipt{}, err }
	terminal, err := decodeProgramHex(execution.TerminalPayload); if err != nil { return VerifiedProgramReceipt{}, err }
	graph, err := decodeProgramHex(execution.CallGraph); if err != nil { return VerifiedProgramReceipt{}, err }
	verified, err := VerifyReceiptOutcome(receipt, authority); if err != nil { return VerifiedProgramReceipt{}, err }
	outcome := verified.Receipt.ProgramOutcome
	terminalDigest := sha256.Sum256(terminal); graphDigest := sha256.Sum256(graph)
	if verified.Receipt.ActivityID != [32]byte(activity) || verified.Receipt.ModuleID != 9 || verified.Receipt.Operation != 3 || verified.Receipt.ModuleVersion != execution.ModuleVersion || outcome == nil || outcome.ABIVersion != execution.GuestABIVersion || outcome.ResultCode != execution.ResultCode || len(graph) == 0 || terminalDigest != outcome.TerminalPayloadRoot || graphDigest != outcome.CallGraphRoot { return VerifiedProgramReceipt{}, errors.New("program receipt binding failed") }
	return VerifiedProgramReceipt{Verification: verified, TerminalPayload: terminal, CallGraph: graph}, nil
}

func decodeProgramHex(value string) ([]byte, error) { if len(value) > 2*1_048_576 || len(value)%2 != 0 { return nil, errors.New("program evidence exceeds bounds") }; bytes, err := hex.DecodeString(value); if err != nil { return nil, errors.New("program evidence is not hexadecimal") }; return bytes, nil }

func programCallWire(call ProgramCall) map[string]any { return map[string]any{"program_id": hex.EncodeToString(call.ProgramID[:]), "calldata": hex.EncodeToString(call.Calldata), "budget": map[string]any{"fuel": programUint64(call.Budget.Fuel), "fee_limit": call.Budget.FeeLimit.String()}, "capabilities": call.Capabilities, "signed_activity": hex.EncodeToString(call.SignedActivity)} }
func programUint64(value uint64) string { return strconv.FormatUint(value, 10) }

type Programs struct { client *Client }
func NewPrograms(client *Client) (*Programs, error) { if client == nil { return nil, errors.New("client required") }; return &Programs{client: client}, nil }
func (p *Programs) Discover(ctx context.Context, program [32]byte) (ProgramDiscovery, error) { var out ProgramDiscovery; id := hex.EncodeToString(program[:]); err := p.client.call(ctx, PlaneAgent, "program.discover", false, map[string]any{"program_id": id, "requested_verification_level": "sequencer-signed"}, &out, CallOptions{PathParameters: map[string]string{"program_id": id}}); return out, err }
func (p *Programs) Interface(ctx context.Context, program [32]byte) (ProgramInterface, error) { var out ProgramInterface; id := hex.EncodeToString(program[:]); err := p.client.call(ctx, PlaneAgent, "program.interface", false, map[string]any{"program_id": id, "requested_verification_level": "sequencer-signed"}, &out, CallOptions{PathParameters: map[string]string{"program_id": id}}); return out, err }
func (p *Programs) Simulate(ctx context.Context, call ProgramCall) (ProgramSimulation, error) { if err := validateProgramCall(call); err != nil { return ProgramSimulation{}, err }; var out ProgramSimulation; err := p.client.call(ctx, PlaneAgent, "program.simulate", false, programCallWire(call), &out, CallOptions{}); return out, err }
func (p *Programs) Submit(ctx context.Context, call ProgramCall, key IdempotencyKey) (ProgramSubmission, error) { if err := validateProgramCall(call); err != nil { return ProgramSubmission{}, err }; var out ProgramSubmission; err := p.client.call(ctx, PlaneAgent, "program.call", true, programCallWire(call), &out, CallOptions{IdempotencyKey: key}); return out, err }
func (p *Programs) Receipt(ctx context.Context, key IdempotencyKey, expectedActivity [32]byte) (ProgramSubmission, error) { var out ProgramSubmission; activity := hex.EncodeToString(expectedActivity[:]); err := p.client.call(ctx, PlaneAgent, "program.receipt", false, map[string]any{"idempotency_key": key.String(), "expected_activity_id": activity, "requested_verification_level": "sequencer-signed"}, &out, CallOptions{PathParameters: map[string]string{"idempotency_key": key.String()}}); if err == nil && out.ActivityID != activity { err = errors.New("program receipt selector binding failed") }; return out, err }
func (p *Programs) Activity(ctx context.Context, activity [32]byte) (ProgramSubmission, error) { var out ProgramSubmission; id := hex.EncodeToString(activity[:]); err := p.client.call(ctx, PlaneAgent, "program.activity", false, map[string]any{"activity_id": id, "requested_verification_level": "sequencer-signed"}, &out, CallOptions{PathParameters: map[string]string{"activity_id": id}}); return out, err }
func PlatformSDKPrograms() string { return "receipt-verified-program-operations-v1" }
