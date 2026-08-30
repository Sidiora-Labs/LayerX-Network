// Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.

package layerx

const ProgramsModuleID uint16 = 9

const (
	ProgramOutcomeTagV1 uint32 = 0x50524731
	ProgramOutcomeTagV2 uint32 = 0x50524732
	ProgramOutcomeTagV3 uint32 = 0x50524733
)

type ReceiptCheck string

const (
	ReceiptCheckDecode             ReceiptCheck = "decode"
	ReceiptCheckCanonicalEncoding  ReceiptCheck = "canonical-encoding"
	ReceiptCheckReceiptShape       ReceiptCheck = "receipt-shape"
	ReceiptCheckMissingSignature   ReceiptCheck = "missing-signature"
	ReceiptCheckProtocolVersion    ReceiptCheck = "protocol-version"
	ReceiptCheckResultCode         ReceiptCheck = "result-code"
	ReceiptCheckOperation          ReceiptCheck = "operation"
	ReceiptCheckActivityID         ReceiptCheck = "activity-id"
	ReceiptCheckGlobalSequence     ReceiptCheck = "global-sequence"
	ReceiptCheckModuleID           ReceiptCheck = "module-id"
	ReceiptCheckModuleVersion      ReceiptCheck = "module-version"
	ReceiptCheckTimestamp          ReceiptCheck = "timestamp"
	ReceiptCheckBatchID            ReceiptCheck = "batch-id"
	ReceiptCheckAsset              ReceiptCheck = "asset"
	ReceiptCheckPreviousStateRoot  ReceiptCheck = "previous-state-root"
	ReceiptCheckResultingStateRoot ReceiptCheck = "resulting-state-root"
	ReceiptCheckDebitBalance       ReceiptCheck = "debit-balance"
	ReceiptCheckCreditBalance      ReceiptCheck = "credit-balance"
	ReceiptCheckProgramOutcome     ReceiptCheck = "program-outcome"
	ReceiptCheckSequencerSignature ReceiptCheck = "sequencer-signature"
)

var RequiredNonzeroChecks = [...]ReceiptCheck{
	ReceiptCheckGlobalSequence,
	ReceiptCheckModuleID,
	ReceiptCheckModuleVersion,
	ReceiptCheckTimestamp,
	ReceiptCheckActivityID,
	ReceiptCheckResultingStateRoot,
}
