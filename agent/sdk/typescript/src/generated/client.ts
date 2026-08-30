// Generated from the LayerX Agent API schema. Do not hand-edit.

const PACKAGE_METADATA = Object.freeze({
  name: "@sidiora/layerx-sdk",
  version: "0.1.0",
  contractMajor: 1,
});

export function layerx_sdk_ts_package(): typeof PACKAGE_METADATA {
  return PACKAGE_METADATA;
}

export type Amount = bigint;
export function parseAmount(value: string): Amount {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new RangeError("invalid Amount");
  const parsed = BigInt(value);
  if (parsed > 340282366920938463463374607431768211455n) throw new RangeError("Amount out of range");
  return parsed;
}
export type BudgetLimit = bigint;
export function parseBudgetLimit(value: string): BudgetLimit {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new RangeError("invalid BudgetLimit");
  const parsed = BigInt(value);
  if (parsed > 340282366920938463463374607431768211455n) throw new RangeError("BudgetLimit out of range");
  return parsed;
}
export type Sequence = bigint;
export function parseSequence(value: string): Sequence {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new RangeError("invalid Sequence");
  const parsed = BigInt(value);
  if (parsed > 18446744073709551615n) throw new RangeError("Sequence out of range");
  return parsed;
}
export type TimestampSeconds = bigint;
export function parseTimestampSeconds(value: string): TimestampSeconds {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new RangeError("invalid TimestampSeconds");
  const parsed = BigInt(value);
  if (parsed > 18446744073709551615n) throw new RangeError("TimestampSeconds out of range");
  return parsed;
}

export enum VerificationLevel {
  Unverified = 0,
  SequencerSigned = 1,
  BatchIncluded = 2,
  StateProven = 3,
  CheckpointFinalised = 4,
  SettlementAnchored = 5,
}

export type SubmissionState =
  | { kind: "Unknown" }
  | { kind: "Executed"; receiptRef: string }
  | { kind: "Failed"; protocolResultCode: number }
  | { kind: "Pending"; stage: string };

export interface VerifiedRead<T> {
  value: T;
  achievedVerificationLevel: VerificationLevel;
  freshness: { chainHead: bigint; latestBatch: string; latestCheckpoint: string; valueSequence: bigint };
}

export function requireVerified<T>(requested: VerificationLevel, read: VerifiedRead<T>): VerifiedRead<T> {
  if (read.achievedVerificationLevel === VerificationLevel.Unverified) {
    throw new Error("unverified_read");
  }
  if (read.achievedVerificationLevel < requested) {
    throw new Error(`verification_below_requested:${requested}:${read.achievedVerificationLevel}`);
  }
  return read;
}

export interface IdempotentMutation<T> {
  requestId: bigint;
  key: Uint8Array;
  bodyDigest: Uint8Array;
  operation: T;
}

export type ErrorClass = "TransportFailure" | "Deadline" | "ProtocolIncompatibility" | "UnavailableCapability" | "CoreRejection" | "VerificationFailure" | "PolicyRefusal" | "CapabilityRefusal" | "BudgetRefusal" | "RateLimit" | "IdempotencyConflict" | "InternalFault";
export interface ApiError {
  errorClass: ErrorClass;
  protocolResultCode: number | null;
  retriable: boolean;
  requestId: bigint;
  reason: string;
}
export type Operation = "agent.register" | "approval.approve" | "approval.get" | "approval.list" | "approval.reject" | "availability.fetch" | "budget.create" | "budget.fund" | "budget.list" | "budget.reconciliation" | "budget.revoke" | "capability.attenuate" | "capability.create" | "capability.list" | "capability.revoke" | "export.offline" | "prepare" | "program.activity" | "program.call" | "program.discover" | "program.interface" | "program.receipt" | "program.simulate" | "project" | "read.account" | "read.balance" | "read.batch" | "read.checkpoint" | "read.history" | "read.module_state" | "read.proof_bundle" | "session.close" | "session.list" | "session.open" | "session.refresh" | "sign" | "submit" | "subscription.acknowledge" | "subscription.create" | "subscription.delete" | "subscription.health" | "subscription.list" | "subscription.pause" | "subscription.resume" | "track" | "wait";

export const APPROVAL_CONTRACT_INTRODUCED = "1.1" as const;
export const APPROVAL_ENFORCEMENT_NOTICE = "An approval hold is a daemon-enforced restriction. It confers no protocol authority, and bypassing the daemon bypasses the restriction." as const;
export const APPROVAL_STATES = ["Held", "Granted", "Rejected", "Expired", "Defective"] as const;
export const APPROVAL_DECISION_OUTCOMES = ["Granted", "Rejected", "Expired", "Defective", "AlreadyDecided", "Conflict"] as const;
export const APPROVAL_EVENT_KINDS = ["Created", "Granted", "Rejected", "Expired", "Defective"] as const;

export type ApprovalState = (typeof APPROVAL_STATES)[number];
export type ApprovalDecisionOutcome = (typeof APPROVAL_DECISION_OUTCOMES)[number];
export type ApprovalEventKind = (typeof APPROVAL_EVENT_KINDS)[number];

export interface StructuredActivityDisclosure {
  canonicalDigest: string;
  activityType: string;
  actor: string;
  authority: string;
  counterparties: readonly string[];
  amounts: readonly Amount[];
  asset: string;
  feeLimit: Amount;
  expiry: TimestampSeconds;
  idempotencyKey: string;
}

export interface HoldReason { code: string; message: string }
export interface ApprovalRecord {
  approvalId: string;
  tenant: string;
  heldActivity: StructuredActivityDisclosure;
  canonicalBytesDigest: string;
  holdReason: HoldReason;
  createdAt: TimestampSeconds;
  expiresAt: TimestampSeconds;
  state: ApprovalState;
  enforcement: "daemon_enforced";
  authorityNotice: typeof APPROVAL_ENFORCEMENT_NOTICE;
}
export interface ApprovalPage { approvals: readonly ApprovalRecord[]; nextCursor: string | null }
export interface ApprovalListRequest { tenant: string; cursor: string | null; pageLimit: number }
export interface ApprovalGetRequest { tenant: string; approvalId: string }
export interface ApprovalApproveRequest { tenant: string; approvalId: string; idempotencyKey: string }
export interface ApprovalRejectRequest { tenant: string; approvalId: string; idempotencyKey: string; reason: string }
export interface ApprovalDecision {
  outcome: ApprovalDecisionOutcome;
  submissionRef: string | null;
  winningOutcome: ApprovalDecisionOutcome | null;
  enforcement: "daemon_enforced";
  authorityNotice: typeof APPROVAL_ENFORCEMENT_NOTICE;
}
export interface ApprovalLifecycleEvent {
  eventId: string;
  tenant: string;
  approvalId: string;
  kind: ApprovalEventKind;
  at: TimestampSeconds;
  recordDigest: string;
  holdReason?: HoldReason;
  expiresAt?: TimestampSeconds;
  submissionRef?: string;
  reason?: string;
  deterministicExpiry?: boolean;
  defectCode?: string;
}

export interface Transport {
  call<TRequest, TResponse>(operation: Operation, request: TRequest): Promise<TResponse>;
}

export class Client {
  public constructor(private readonly transport: Transport) {}

  public call<TRequest, TResponse>(operation: Operation, request: TRequest): Promise<TResponse> {
    return this.transport.call<TRequest, TResponse>(operation, request);
  }

  public approvalList(request: ApprovalListRequest): Promise<ApprovalPage> {
    return this.call("approval.list", request);
  }

  public approvalGet(request: ApprovalGetRequest): Promise<ApprovalRecord> {
    return this.call("approval.get", request);
  }

  public approvalApprove(request: ApprovalApproveRequest): Promise<ApprovalDecision> {
    return this.call("approval.approve", request);
  }

  public approvalReject(request: ApprovalRejectRequest): Promise<ApprovalDecision> {
    return this.call("approval.reject", request);
  }
}
