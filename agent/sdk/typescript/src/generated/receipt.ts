// Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.

export const PROGRAMS_MODULE_ID = 9;
export const PROGRAM_OUTCOME_TAGS = [0x50524731, 0x50524732, 0x50524733] as const;

export enum ReceiptFailureCode {
  Decode = "decode",
  CanonicalEncoding = "canonical-encoding",
  ReceiptShape = "receipt-shape",
  MissingSignature = "missing-signature",
  ProtocolVersion = "protocol-version",
  ResultCode = "result-code",
  Operation = "operation",
  ActivityId = "activity-id",
  GlobalSequence = "global-sequence",
  ModuleId = "module-id",
  ModuleVersion = "module-version",
  Timestamp = "timestamp",
  BatchId = "batch-id",
  Asset = "asset",
  PreviousStateRoot = "previous-state-root",
  ResultingStateRoot = "resulting-state-root",
  DebitBalance = "debit-balance",
  CreditBalance = "credit-balance",
  ProgramOutcome = "program-outcome",
  SequencerSignature = "sequencer-signature",
}

export const REQUIRED_NONZERO_CHECKS = Object.freeze([
  ReceiptFailureCode.GlobalSequence,
  ReceiptFailureCode.ModuleId,
  ReceiptFailureCode.ModuleVersion,
  ReceiptFailureCode.Timestamp,
  ReceiptFailureCode.ActivityId,
  ReceiptFailureCode.ResultingStateRoot,
]);
