export type ExplorerVerificationLevel =
  | "unverified"
  | "sequencer-signed"
  | "batch-included"
  | "state-proven"
  | "checkpoint-finalised"
  | "settlement-anchored";

export interface ExplorerFreshness {
  readonly observedChainSequence: string;
  readonly observedSealedBatch: string;
  readonly observedFinalisedCheckpoint: string;
  readonly indexedBatch?: string;
  readonly indexedCheckpoint?: string;
  readonly batchesBehind: string;
  readonly current: boolean;
}

export interface CheckpointRecord {
  readonly checkpointId: string;
  readonly batchNumber: string;
  readonly firstSequence: string;
  readonly lastSequence: string;
  readonly achievedSignatures: string;
  readonly requiredSignatures: string;
  readonly verificationLevel: ExplorerVerificationLevel;
}

export interface BatchRecord {
  readonly batchNumber: string;
  readonly totalAvailabilityBytes: string;
  readonly activityCount: string;
  readonly receiptCount: string;
  readonly eventCount: string;
  readonly checkpointId?: string;
  readonly verificationLevel: ExplorerVerificationLevel;
}

export interface ReceiptRecord {
  readonly receiptId: string;
  readonly batchNumber: string;
  readonly ordinal: string;
  readonly canonicalBytes: string;
  readonly verificationLevel: ExplorerVerificationLevel;
}

export interface AccountActivityRecord {
  readonly receiptId: string;
  readonly receiptDigest: string;
  readonly batchNumber: string;
  readonly globalSequence: string;
  readonly activityId: string;
  readonly operation: string;
  readonly resultCode: string;
  readonly asset: string;
  readonly amount: string;
  readonly from: string;
  readonly to: string;
  readonly verificationLevel: ExplorerVerificationLevel;
}

export type ProgramLifecycle = "active" | "deprecated" | "tombstoned";

export type ProgramUpgradePolicy =
  | Readonly<{ kind: "immutable" }>
  | Readonly<{ kind: "upgradeable"; authority: string }>;

export type ProgramSourceStatus =
  | Readonly<{ status: "unpublished" }>
  | Readonly<{ status: "verified"; sourceDigest: string; environmentDigest: string }>
  | Readonly<{ status: "mismatch"; expected: string; reproduced: string }>;

export interface ProgramVersionRecord {
  readonly version: string;
  readonly codeHash: string;
  readonly abiVersion: string;
  readonly interfaceDigest?: string;
  readonly source: ProgramSourceStatus;
}

export interface ProgramValueAccountRecord {
  readonly account: string;
  readonly asset: string;
  readonly balance: string;
  readonly frozen: boolean;
}

export interface ProgramRecord {
  readonly program: string;
  readonly upgradePolicy: ProgramUpgradePolicy;
  readonly lifecycle: ProgramLifecycle;
  readonly versions: readonly ProgramVersionRecord[];
  readonly valueAccounts: readonly ProgramValueAccountRecord[];
  readonly observedSequence: string;
  readonly observedAt: string;
  readonly receiptDigest: string;
  readonly stateRoot: string;
}

export interface ExplorerPage<T> {
  readonly items: readonly T[];
  readonly nextBefore?: string;
  readonly freshness: ExplorerFreshness;
}

export interface ExplorerRecord<T> {
  readonly value?: T;
  readonly freshness: ExplorerFreshness;
}

export interface EvidenceVerificationReport {
  readonly kind: "receipt" | "state-inclusion";
  readonly achievedLevel: ExplorerVerificationLevel;
  readonly receiptDigest?: string;
  readonly headerDigest?: string;
  readonly proofRoot?: string;
  readonly freshness?: ExplorerFreshness;
  readonly mirror?: MirrorVerificationProvenance;
}

export interface MirrorVerificationProvenance {
  readonly sourceId: string;
  readonly target: string;
  readonly canonicalPosition: string;
  readonly provenance: "canonical" | "reorged";
  readonly latestBatch?: string;
  readonly batchLag: Readonly<{ kind: "known"; batches: string }> | Readonly<{ kind: "unknown" }>;
  readonly failoverCount: string;
  readonly agreeingSources: string;
  readonly checkpointLevel: "unavailable";
  readonly degraded: boolean;
}

type JsonRecord = Readonly<Record<string, unknown>>;

function record(value: unknown, at: string): JsonRecord {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new TypeError(`${at} must be an object`);
  }
  return value as JsonRecord;
}

function text(value: unknown, at: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${at} must be a non-empty string`);
  }
  return value;
}

function decimal(value: unknown, at: string): string {
  const candidate = text(value, at);
  if (!/^(?:0|[1-9]\d*)$/u.test(candidate)) {
    throw new TypeError(`${at} must be an unsigned decimal string`);
  }
  return candidate;
}

function signedDecimal(value: unknown, at: string): string {
  const candidate = text(value, at);
  if (!/^(?:0|-?[1-9]\d*)$/u.test(candidate)) {
    throw new TypeError(`${at} must be a signed decimal string`);
  }
  return candidate;
}

function hex(value: unknown, at: string): string {
  const candidate = text(value, at).toLowerCase();
  if (!/^[0-9a-f]{64}$/u.test(candidate)) {
    throw new TypeError(`${at} must be a 32-byte lowercase hex identifier`);
  }
  return candidate;
}

function optionalHex(value: unknown, at: string): string | undefined {
  return value === undefined || value === null ? undefined : hex(value, at);
}

function boolean(value: unknown, at: string): boolean {
  if (typeof value !== "boolean") {
    throw new TypeError(`${at} must be a boolean`);
  }
  return value;
}

function verificationLevel(value: unknown, at: string): ExplorerVerificationLevel {
  const candidate = text(value, at);
  if (
    candidate !== "unverified"
    && candidate !== "sequencer-signed"
    && candidate !== "batch-included"
    && candidate !== "state-proven"
    && candidate !== "checkpoint-finalised"
    && candidate !== "settlement-anchored"
  ) {
    throw new TypeError(`${at} is not a declared verification level`);
  }
  return candidate;
}

export function decodeFreshness(value: unknown, at = "freshness"): ExplorerFreshness {
  const item = record(value, at);
  const indexedCheckpoint = optionalHex(item.indexed_checkpoint, `${at}.indexed_checkpoint`);
  return Object.freeze({
    observedChainSequence: decimal(item.observed_chain_sequence, `${at}.observed_chain_sequence`),
    observedSealedBatch: decimal(item.observed_sealed_batch, `${at}.observed_sealed_batch`),
    observedFinalisedCheckpoint: hex(
      item.observed_finalised_checkpoint,
      `${at}.observed_finalised_checkpoint`,
    ),
    ...(indexedCheckpoint === undefined ? {} : { indexedCheckpoint }),
    ...(item.indexed_batch === undefined || item.indexed_batch === null
      ? {}
      : { indexedBatch: decimal(item.indexed_batch, `${at}.indexed_batch`) }),
    batchesBehind: decimal(item.batches_behind, `${at}.batches_behind`),
    current: boolean(item.current, `${at}.current`),
  });
}

export function decodeCheckpoint(value: unknown, at = "checkpoint"): CheckpointRecord {
  const item = record(value, at);
  return Object.freeze({
    checkpointId: hex(item.checkpoint_id, `${at}.checkpoint_id`),
    batchNumber: decimal(item.batch_number, `${at}.batch_number`),
    firstSequence: decimal(item.first_sequence, `${at}.first_sequence`),
    lastSequence: decimal(item.last_sequence, `${at}.last_sequence`),
    achievedSignatures: decimal(item.achieved_signatures, `${at}.achieved_signatures`),
    requiredSignatures: decimal(item.required_signatures, `${at}.required_signatures`),
    verificationLevel: verificationLevel(item.verification_level, `${at}.verification_level`),
  });
}

export function decodeBatch(value: unknown, at = "batch"): BatchRecord {
  const item = record(value, at);
  const checkpointId = optionalHex(item.checkpoint_id, `${at}.checkpoint_id`);
  return Object.freeze({
    batchNumber: decimal(item.batch_number, `${at}.batch_number`),
    totalAvailabilityBytes: decimal(
      item.total_availability_bytes,
      `${at}.total_availability_bytes`,
    ),
    activityCount: decimal(item.activity_count, `${at}.activity_count`),
    receiptCount: decimal(item.receipt_count, `${at}.receipt_count`),
    eventCount: decimal(item.event_count, `${at}.event_count`),
    ...(checkpointId === undefined ? {} : { checkpointId }),
    verificationLevel: verificationLevel(item.verification_level, `${at}.verification_level`),
  });
}

export function decodeReceipt(value: unknown, at = "receipt"): ReceiptRecord {
  const item = record(value, at);
  const canonicalBytes = text(item.canonical_bytes, `${at}.canonical_bytes`);
  if (!/^[A-Za-z0-9_-]+={0,2}$/u.test(canonicalBytes) || canonicalBytes.length > 1_500_000) {
    throw new TypeError(`${at}.canonical_bytes must be bounded base64url`);
  }
  return Object.freeze({
    receiptId: hex(item.receipt_id, `${at}.receipt_id`),
    batchNumber: decimal(item.batch_number, `${at}.batch_number`),
    ordinal: decimal(item.ordinal, `${at}.ordinal`),
    canonicalBytes,
    verificationLevel: verificationLevel(item.verification_level, `${at}.verification_level`),
  });
}

export function decodeAccountActivity(
  value: unknown,
  at = "account_activity",
): AccountActivityRecord {
  const item = record(value, at);
  return Object.freeze({
    receiptId: hex(item.receipt_id, `${at}.receipt_id`),
    receiptDigest: hex(item.receipt_digest, `${at}.receipt_digest`),
    batchNumber: decimal(item.batch_number, `${at}.batch_number`),
    globalSequence: decimal(item.global_sequence, `${at}.global_sequence`),
    activityId: hex(item.activity_id, `${at}.activity_id`),
    operation: decimal(item.operation, `${at}.operation`),
    resultCode: signedDecimal(item.result_code, `${at}.result_code`),
    asset: hex(item.asset, `${at}.asset`),
    amount: decimal(item.amount, `${at}.amount`),
    from: hex(item.from, `${at}.from`),
    to: hex(item.to, `${at}.to`),
    verificationLevel: verificationLevel(item.verification_level, `${at}.verification_level`),
  });
}

function decodeProgramSource(value: unknown, at: string): ProgramSourceStatus {
  const item = record(value, at);
  const status = text(item.status, `${at}.status`);
  if (status === "unpublished") {
    return Object.freeze({ status });
  }
  if (status === "verified") {
    return Object.freeze({
      status,
      sourceDigest: hex(item.source_digest, `${at}.source_digest`),
      environmentDigest: hex(item.environment_digest, `${at}.environment_digest`),
    });
  }
  if (status === "mismatch") {
    return Object.freeze({
      status,
      expected: hex(item.expected, `${at}.expected`),
      reproduced: hex(item.reproduced, `${at}.reproduced`),
    });
  }
  throw new TypeError(`${at}.status is not a declared source status`);
}

export function decodeProgram(value: unknown, at = "program"): ProgramRecord {
  const item = record(value, at);
  const policy = record(item.upgrade_policy, `${at}.upgrade_policy`);
  const policyKind = text(policy.kind, `${at}.upgrade_policy.kind`);
  if (policyKind !== "immutable" && policyKind !== "upgradeable") {
    throw new TypeError(`${at}.upgrade_policy.kind is invalid`);
  }
  const lifecycle = text(item.lifecycle, `${at}.lifecycle`);
  if (lifecycle !== "active" && lifecycle !== "deprecated" && lifecycle !== "tombstoned") {
    throw new TypeError(`${at}.lifecycle is invalid`);
  }
  if (!Array.isArray(item.versions) || item.versions.length === 0 || item.versions.length > 1_024) {
    throw new TypeError(`${at}.versions must be a bounded non-empty array`);
  }
  if (!Array.isArray(item.value_accounts) || item.value_accounts.length > 1_024) {
    throw new TypeError(`${at}.value_accounts must be a bounded array`);
  }
  return Object.freeze({
    program: hex(item.program, `${at}.program`),
    upgradePolicy: policyKind === "immutable"
      ? Object.freeze({ kind: "immutable" as const })
      : Object.freeze({
          kind: "upgradeable" as const,
          authority: hex(policy.authority, `${at}.upgrade_policy.authority`),
        }),
    lifecycle,
    versions: Object.freeze(item.versions.map((candidate, index) => {
      const version = record(candidate, `${at}.versions[${String(index)}]`);
      return Object.freeze({
        version: decimal(version.version, `${at}.versions[${String(index)}].version`),
        codeHash: hex(version.code_hash, `${at}.versions[${String(index)}].code_hash`),
        abiVersion: decimal(version.abi_version, `${at}.versions[${String(index)}].abi_version`),
        ...(version.interface_digest === null || version.interface_digest === undefined
          ? {}
          : { interfaceDigest: hex(version.interface_digest, `${at}.versions[${String(index)}].interface_digest`) }),
        source: decodeProgramSource(version.source, `${at}.versions[${String(index)}].source`),
      });
    })),
    valueAccounts: Object.freeze(item.value_accounts.map((candidate, index) => {
      const account = record(candidate, `${at}.value_accounts[${String(index)}]`);
      return Object.freeze({
        account: hex(account.account, `${at}.value_accounts[${String(index)}].account`),
        asset: hex(account.asset, `${at}.value_accounts[${String(index)}].asset`),
        balance: decimal(account.balance, `${at}.value_accounts[${String(index)}].balance`),
        frozen: boolean(account.frozen, `${at}.value_accounts[${String(index)}].frozen`),
      });
    })),
    observedSequence: decimal(item.observed_sequence, `${at}.observed_sequence`),
    observedAt: decimal(item.observed_at, `${at}.observed_at`),
    receiptDigest: hex(item.receipt_digest, `${at}.receipt_digest`),
    stateRoot: hex(item.state_root, `${at}.state_root`),
  });
}

export function decodePage<T>(
  value: unknown,
  decodeItem: (value: unknown, at: string) => T,
  at = "page",
): ExplorerPage<T> {
  const item = record(value, at);
  if (!Array.isArray(item.items)) {
    throw new TypeError(`${at}.items must be an array`);
  }
  return Object.freeze({
    items: Object.freeze(item.items.map((entry, index) => decodeItem(entry, `${at}.items[${String(index)}]`))),
    ...(item.next_before === undefined || item.next_before === null
      ? {}
      : { nextBefore: decimal(item.next_before, `${at}.next_before`) }),
    freshness: decodeFreshness(item.freshness, `${at}.freshness`),
  });
}

export function decodeRecord<T>(
  value: unknown,
  decodeValue: (value: unknown, at: string) => T,
  at = "record",
): ExplorerRecord<T> {
  const item = record(value, at);
  return Object.freeze({
    ...(item.value === undefined || item.value === null
      ? {}
      : { value: decodeValue(item.value, `${at}.value`) }),
    freshness: decodeFreshness(item.freshness, `${at}.freshness`),
  });
}

export function decodeVerificationReport(
  value: unknown,
  at = "verification",
): EvidenceVerificationReport {
  const item = record(value, at);
  const kind = text(item.kind, `${at}.kind`);
  if (kind !== "receipt" && kind !== "state-inclusion") {
    throw new TypeError(`${at}.kind is not supported`);
  }
  const receiptDigest = optionalHex(item.receipt_digest, `${at}.receipt_digest`);
  const headerDigest = optionalHex(item.header_digest, `${at}.header_digest`);
  const proofRoot = optionalHex(item.proof_root, `${at}.proof_root`);
  const freshness = item.freshness === undefined ? undefined : decodeFreshness(item.freshness, `${at}.freshness`);
  const mirror = item.mirror === undefined ? undefined : decodeMirrorProvenance(item.mirror, `${at}.mirror`);
  if ((freshness === undefined) === (mirror === undefined)) {
    throw new TypeError(`${at} must carry exactly one freshness source`);
  }
  return Object.freeze({
    kind,
    achievedLevel: verificationLevel(item.achieved_level, `${at}.achieved_level`),
    ...(receiptDigest === undefined ? {} : { receiptDigest }),
    ...(headerDigest === undefined ? {} : { headerDigest }),
    ...(proofRoot === undefined ? {} : { proofRoot }),
    ...(freshness === undefined ? {} : { freshness }),
    ...(mirror === undefined ? {} : { mirror }),
  });
}

export function decodeMirrorProvenance(value:unknown,at="mirror"):MirrorVerificationProvenance{const item=record(value,at);const provenance=text(item.provenance,`${at}.provenance`);if(provenance!=="canonical"&&provenance!=="reorged")throw new TypeError(`${at}.provenance is invalid`);if(item.checkpoint_level!=="unavailable")throw new TypeError(`${at}.checkpoint_level is invalid`);const lag=record(item.batch_lag,`${at}.batch_lag`);const lagKind=text(lag.kind,`${at}.batch_lag.kind`);if(lagKind!=="known"&&lagKind!=="unknown")throw new TypeError(`${at}.batch_lag.kind is invalid`);const latest=item.latest_batch===undefined||item.latest_batch===null?undefined:decimal(item.latest_batch,`${at}.latest_batch`);return Object.freeze({sourceId:text(item.source_id,`${at}.source_id`),target:text(item.target,`${at}.target`),canonicalPosition:text(item.canonical_position,`${at}.canonical_position`),provenance,...(latest===undefined?{}:{latestBatch:latest}),batchLag:lagKind==="known"?Object.freeze({kind:"known"as const,batches:decimal(lag.batches,`${at}.batch_lag.batches`)}):Object.freeze({kind:"unknown"as const}),failoverCount:decimal(item.failover_count,`${at}.failover_count`),agreeingSources:decimal(item.agreeing_sources,`${at}.agreeing_sources`),checkpointLevel:"unavailable",degraded:boolean(item.degraded,`${at}.degraded`)});}

export function encodeFreshness(freshness: ExplorerFreshness): Readonly<Record<string, unknown>> {
  return Object.freeze({
    observed_chain_sequence: freshness.observedChainSequence,
    observed_sealed_batch: freshness.observedSealedBatch,
    observed_finalised_checkpoint: freshness.observedFinalisedCheckpoint,
    indexed_batch: freshness.indexedBatch ?? null,
    indexed_checkpoint: freshness.indexedCheckpoint ?? null,
    batches_behind: freshness.batchesBehind,
    current: freshness.current,
  });
}

export function encodeVerificationReport(
  report: EvidenceVerificationReport,
): Readonly<Record<string, unknown>> {
  return Object.freeze({
    kind: report.kind,
    achieved_level: report.achievedLevel,
    receipt_digest: report.receiptDigest ?? null,
    header_digest: report.headerDigest ?? null,
    proof_root: report.proofRoot ?? null,
    ...(report.freshness===undefined?{}:{freshness:encodeFreshness(report.freshness)}),
    ...(report.mirror===undefined?{}:{mirror:{source_id:report.mirror.sourceId,target:report.mirror.target,canonical_position:report.mirror.canonicalPosition,provenance:report.mirror.provenance,latest_batch:report.mirror.latestBatch??null,batch_lag:report.mirror.batchLag.kind==="known"?{kind:"known",batches:report.mirror.batchLag.batches}:{kind:"unknown"},failover_count:report.mirror.failoverCount,agreeing_sources:report.mirror.agreeingSources,checkpoint_level:report.mirror.checkpointLevel,degraded:report.mirror.degraded}}),
  });
}

export function validExplorerIdentifier(value: string): boolean {
  return /^[0-9a-fA-F]{64}$/u.test(value);
}

export function validExplorerCoordinate(value: string): boolean {
  return /^(?:0|[1-9]\d*)$/u.test(value);
}

export function explorer(): Readonly<{ publicOnly: true; maximumPageSize: 100 }> {
  return Object.freeze({ publicOnly: true, maximumPageSize: 100 });
}

export function human_web_explorer() {
  return explorer();
}
