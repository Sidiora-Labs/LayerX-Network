import { PlatformSdkError } from "./production.js";

const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");
const MERKLE_INTERNAL_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-internal\0");
const BATCH_HEADER_DOMAIN = new TextEncoder().encode("LXP/v1/batch-header\0");
const RECEIPT_DOMAIN = new TextEncoder().encode("LXP/v1/receipt\0");
const CHECKPOINT_DOMAIN = new TextEncoder().encode("LXP/v1/checkpoint-certificate\0");
const GUARANTOR_ATTESTATION_DOMAIN = new TextEncoder().encode("LXP/v1/guarantor-attestation\0");
const BATCH_HEADER_BYTES = 354;
const MAX_MESSAGE_BYTES = 1_048_576;
const MAX_EFFECTS = 512;
const MAX_EFFECT_BODY = 256;
const MAX_U128 = 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffffn;
const ALL_AVAILABILITY_CLASSES = 0x1f;
const PROGRAM_OUTCOME_V1 = 0x5052_4731;
const PROGRAM_OUTCOME_V2 = 0x5052_4732;
const PROGRAM_OUTCOME_V3 = 0x5052_4733;

export interface MerkleProof {
  readonly leafIndex: number;
  readonly leafCount: number;
  readonly siblings: readonly Uint8Array[];
}

export interface BatchHeader {
  readonly protocolVersion: number;
  readonly networkId: number;
  readonly epoch: bigint;
  readonly batchNumber: bigint;
  readonly firstSequence: bigint;
  readonly lastSequence: bigint;
  readonly previousStateRoot: Uint8Array;
  readonly resultingStateRoot: Uint8Array;
  readonly activityMerkleRoot: Uint8Array;
  readonly receiptMerkleRoot: Uint8Array;
  readonly eventMerkleRoot: Uint8Array;
  readonly dataAvailabilityRoot: Uint8Array;
  readonly oracleRoot: Uint8Array;
  readonly timestampMs: bigint;
  readonly sequencerId: Uint8Array;
}

export interface SequencerAuthorization {
  readonly sequencerId: Uint8Array;
  readonly publicKey: Uint8Array;
  readonly firstBatchNumber: bigint;
  readonly lastBatchNumber: bigint;
}

export type InclusionKind = "activity" | "receipt" | "event" | "state";

export interface InclusionVerification {
  readonly level: "batch-included" | "state-proven";
  readonly header: BatchHeader;
  readonly headerDigest: Uint8Array;
  readonly root: Uint8Array;
}

export interface CheckpointAttestation {
  readonly protocolVersion: number;
  readonly networkId: number;
  readonly paxeerChainId: bigint;
  readonly settlementContract: Uint8Array;
  readonly epoch: bigint;
  readonly checkpointId: Uint8Array;
  readonly checkpointHash: Uint8Array;
  readonly guarantorId: Uint8Array;
  readonly batchNumber: bigint;
  readonly dataAvailabilityRoot: Uint8Array;
  readonly replayed: boolean;
  readonly dataPossessed: boolean;
  readonly availabilityClassMask: number;
  readonly attestedAtMs: bigint;
  readonly signer: Uint8Array;
  readonly signature: Uint8Array;
  readonly signatureV: number;
}

export interface GuarantorKey {
  readonly guarantorId: Uint8Array;
  readonly publicKey: Uint8Array;
  readonly bonded: boolean;
}

export interface CheckpointCertificate {
  readonly canonicalHeader: Uint8Array;
  readonly validityProof: Uint8Array;
  readonly attestations: readonly CheckpointAttestation[];
  readonly threshold: number;
  readonly settlementReference?: Uint8Array;
}

export interface CheckpointVerificationInput {
  readonly certificate: CheckpointCertificate;
  readonly bondedSet: readonly GuarantorKey[];
  readonly registeredCheckpointId: Uint8Array;
  readonly expectedPaxeerChainId: bigint;
  readonly expectedSettlementContract: Uint8Array;
  readonly registeredSettlementReference?: Uint8Array;
  readonly availabilityObtained: boolean;
}

export interface CheckpointVerification {
  readonly level: "checkpoint-finalised" | "settlement-anchored";
  readonly checkpointId: Uint8Array;
  readonly achieved: number;
  readonly required: number;
  readonly header: BatchHeader;
}

export interface LocalSignatureVerifier {
  verifyRecoverableSecp256k1(
    publicKey: Uint8Array,
    signature: Uint8Array,
    signatureV: number,
    signer: Uint8Array,
    digest: Uint8Array,
  ): Promise<boolean>;
}

export interface ReceiptEffect {
  readonly moduleId: number;
  readonly ordinal: number;
  readonly eventType: number;
  readonly kind: 1 | 2 | 3;
  readonly monetary: boolean;
  readonly transferSetRoot: Uint8Array;
  readonly body: Uint8Array;
}

export interface ProgramReceiptOutcome {
  readonly encodingVersion: 1 | 2 | 3;
  readonly terminalKind: 1 | 2 | 3;
  readonly resultCode: number;
  readonly runtimeVersion: number;
  readonly abiVersion: number;
  readonly feeScheduleVersion: number;
  readonly meteringScheduleVersion: number;
  readonly cpuFuel: bigint;
  readonly memoryBytes: bigint;
  readonly storageReadBytes: bigint;
  readonly storageWriteBytes: bigint;
  readonly outputValues: number;
  readonly outputBytes: bigint;
  readonly occupancyByteBatches: bigint;
  readonly occupancyFeeUnits: bigint;
  readonly feeSchedulePrices: readonly bigint[];
  readonly occupancyAssetId: Uint8Array;
  readonly occupancyEvidenceDigest: Uint8Array;
  readonly occupancyTransferRoot: Uint8Array;
  readonly feeUnits: bigint;
  readonly callGraphRoot: Uint8Array;
  readonly terminalPayloadRoot: Uint8Array;
  readonly transferRoot: Uint8Array;
}

export interface ProtocolReceipt {
  readonly protocolVersion: number;
  readonly activityId: Uint8Array;
  readonly globalSequence: bigint;
  readonly previousStateRoot: Uint8Array;
  readonly resultingStateRoot: Uint8Array;
  readonly activityRoot: Uint8Array;
  readonly resultCode: number;
  readonly effects: readonly ReceiptEffect[];
  readonly feeCharged: bigint;
  readonly batchId: Uint8Array;
  readonly moduleId: number;
  readonly moduleVersion: number;
  readonly parameterVersion: number;
  readonly operation: number;
  readonly asset: Uint8Array;
  readonly amount: bigint;
  readonly from: Uint8Array;
  readonly fromBalanceBefore: bigint;
  readonly fromBalanceAfter: bigint;
  readonly fromSequence: bigint;
  readonly to: Uint8Array;
  readonly toBalanceBefore: bigint;
  readonly toBalanceAfter: bigint;
  readonly transferSetRoot: Uint8Array;
  readonly authorizationHash: Uint8Array;
  readonly contextHash: Uint8Array;
  readonly timestamp: bigint;
  readonly programOutcome?: ProgramReceiptOutcome;
  readonly sequencerSignature: Uint8Array;
}

export interface AuthorizedReceiptBatch {
  readonly batchId: Uint8Array;
  readonly asset: Uint8Array;
  readonly previousStateRoot: Uint8Array;
  readonly resultingStateRoot: Uint8Array;
  readonly sequencerPublicKey: Uint8Array;
}

export interface ReceiptVerification {
  readonly level: "sequencer-signed";
  readonly receipt: ProtocolReceipt;
  readonly canonicalBytes: Uint8Array;
  readonly receiptDigest: Uint8Array;
}

function verificationFailure(): never {
  throw new PlatformSdkError({ code: "verification-failure", retry: "never" });
}

function exactBytes(value: Uint8Array, length: number): Uint8Array {
  if (value.length !== length) {
    return verificationFailure();
  }
  return value;
}

function arrayBuffer(value: Uint8Array): ArrayBuffer {
  const copy = new ArrayBuffer(value.length);
  new Uint8Array(copy).set(value);
  return copy;
}

function concatenate(...values: readonly Uint8Array[]): Uint8Array {
  const length = values.reduce((total, value) => total + value.length, 0);
  const result = new Uint8Array(length);
  let offset = 0;
  for (const value of values) {
    result.set(value, offset);
    offset += value.length;
  }
  return result;
}

async function sha256(...values: readonly Uint8Array[]): Promise<Uint8Array> {
  const digest = await globalThis.crypto.subtle.digest("SHA-256", arrayBuffer(concatenate(...values)));
  return new Uint8Array(digest);
}

function equal(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}

function proofDepth(leafCount: number): number {
  let count = leafCount;
  let depth = 0;
  while (count > 1) {
    count = Math.ceil(count / 2);
    depth += 1;
  }
  return depth;
}

async function leafHash(bytes: Uint8Array): Promise<Uint8Array> {
  return sha256(MERKLE_LEAF_DOMAIN, bytes);
}

async function nodeHash(left: Uint8Array, right: Uint8Array): Promise<Uint8Array> {
  return sha256(MERKLE_INTERNAL_DOMAIN, exactBytes(left, 32), exactBytes(right, 32));
}

export async function verifyMerkleInclusion(
  canonicalLeaf: Uint8Array,
  proof: MerkleProof,
  expectedRoot: Uint8Array,
): Promise<void> {
  if (
    !Number.isSafeInteger(proof.leafIndex)
    || !Number.isSafeInteger(proof.leafCount)
    || proof.leafCount <= 0
    || proof.leafCount > 0xffff_ffff
    || proof.leafIndex < 0
    || proof.leafIndex > 0xffff_ffff
    || proof.leafIndex >= proof.leafCount
    || proof.siblings.length > 32
    || proof.siblings.length !== proofDepth(proof.leafCount)
  ) {
    return verificationFailure();
  }
  let current = await leafHash(canonicalLeaf);
  let index = proof.leafIndex;
  let levelCount = proof.leafCount;
  for (const siblingValue of proof.siblings) {
    const sibling = exactBytes(siblingValue, 32);
    if ((index ^ 1) >= levelCount && !equal(sibling, current)) {
      return verificationFailure();
    }
    current = index % 2 === 0
      ? await nodeHash(current, sibling)
      : await nodeHash(sibling, current);
    index = Math.floor(index / 2);
    levelCount = Math.ceil(levelCount / 2);
  }
  if (!equal(current, exactBytes(expectedRoot, 32))) {
    return verificationFailure();
  }
}

class Decoder {
  #offset = 0;

  public constructor(private readonly bytes: Uint8Array) {}

  public finish(): void {
    if (this.#offset !== this.bytes.length) {
      return verificationFailure();
    }
  }

  public u8(): number {
    const value = this.bytes[this.#offset];
    if (value === undefined) {
      return verificationFailure();
    }
    this.#offset += 1;
    return value;
  }

  public u16(): number {
    return Number(this.integer(2));
  }

  public u32(): number {
    return Number(this.integer(4));
  }

  public u64(): bigint {
    return this.integer(8);
  }

  public u128(): bigint {
    return this.integer(16);
  }

  public i32(): number {
    const value = this.u32();
    return value > 0x7fff_ffff ? value - 0x1_0000_0000 : value;
  }

  public position(): number {
    return this.#offset;
  }

  public remaining(): number {
    return this.bytes.length - this.#offset;
  }

  public fixed(length: number): Uint8Array {
    const end = this.#offset + length;
    if (!Number.isSafeInteger(end) || end > this.bytes.length) {
      return verificationFailure();
    }
    const value = this.bytes.slice(this.#offset, end);
    this.#offset = end;
    return value;
  }

  public bounded(length: number): Uint8Array {
    if (this.u32() !== length) {
      return verificationFailure();
    }
    return this.fixed(length);
  }

  public boundedAtMost(maximum: number): Uint8Array {
    const length = this.u32();
    if (length > maximum) {
      return verificationFailure();
    }
    return this.fixed(length);
  }

  private integer(length: number): bigint {
    const bytes = this.fixed(length);
    let value = 0n;
    for (const byte of bytes) {
      value = (value << 8n) | BigInt(byte);
    }
    return value;
  }
}

function field(decoder: Decoder, expected: number): void {
  if (decoder.u8() !== expected) {
    return verificationFailure();
  }
}

export function decodeBatchHeader(canonicalHeader: Uint8Array): BatchHeader {
  if (canonicalHeader.length !== BATCH_HEADER_BYTES) {
    return verificationFailure();
  }
  const decoder = new Decoder(canonicalHeader);
  if (decoder.u16() !== 1 || decoder.u16() !== 0x1701 || decoder.u8() !== 15) {
    return verificationFailure();
  }
  field(decoder, 1);
  const protocolVersion = decoder.u16();
  field(decoder, 2);
  const networkId = decoder.u32();
  field(decoder, 3);
  const epoch = decoder.u64();
  field(decoder, 4);
  const batchNumber = decoder.u64();
  field(decoder, 5);
  const firstSequence = decoder.u64();
  field(decoder, 6);
  const lastSequence = decoder.u64();
  field(decoder, 7);
  const previousStateRoot = decoder.bounded(32);
  field(decoder, 8);
  const resultingStateRoot = decoder.bounded(32);
  field(decoder, 9);
  const activityMerkleRoot = decoder.bounded(32);
  field(decoder, 10);
  const receiptMerkleRoot = decoder.bounded(32);
  field(decoder, 11);
  const eventMerkleRoot = decoder.bounded(32);
  field(decoder, 12);
  const dataAvailabilityRoot = decoder.bounded(32);
  field(decoder, 13);
  const oracleRoot = decoder.bounded(32);
  field(decoder, 14);
  const timestampMs = decoder.u64();
  field(decoder, 15);
  const sequencerId = decoder.bounded(32);
  decoder.finish();
  return Object.freeze({
    protocolVersion,
    networkId,
    epoch,
    batchNumber,
    firstSequence,
    lastSequence,
    previousStateRoot,
    resultingStateRoot,
    activityMerkleRoot,
    receiptMerkleRoot,
    eventMerkleRoot,
    dataAvailabilityRoot,
    oracleRoot,
    timestampMs,
    sequencerId,
  });
}

async function verifyEd25519(publicKey: Uint8Array, signature: Uint8Array, digest: Uint8Array): Promise<boolean> {
  try {
    const key = await globalThis.crypto.subtle.importKey(
      "raw",
      arrayBuffer(exactBytes(publicKey, 32)),
      { name: "Ed25519" },
      false,
      ["verify"],
    );
    return await globalThis.crypto.subtle.verify(
      { name: "Ed25519" },
      key,
      arrayBuffer(exactBytes(signature, 64)),
      arrayBuffer(exactBytes(digest, 32)),
    );
  } catch {
    return false;
  }
}

function inclusionRoot(header: BatchHeader, kind: InclusionKind): Uint8Array {
  switch (kind) {
    case "activity":
      return header.activityMerkleRoot;
    case "receipt":
      return header.receiptMerkleRoot;
    case "event":
      return header.eventMerkleRoot;
    case "state":
      return header.resultingStateRoot;
  }
}

export async function verifyBatchInclusion(
  kind: InclusionKind,
  canonicalLeaf: Uint8Array,
  proof: MerkleProof,
  canonicalHeader: Uint8Array,
  headerSignature: Uint8Array,
  authorization: SequencerAuthorization,
): Promise<InclusionVerification> {
  const header = decodeBatchHeader(canonicalHeader);
  if (
    header.batchNumber < authorization.firstBatchNumber
    || header.batchNumber > authorization.lastBatchNumber
    || !equal(header.sequencerId, exactBytes(authorization.sequencerId, 32))
  ) {
    return verificationFailure();
  }
  const digest = await sha256(BATCH_HEADER_DOMAIN, canonicalHeader);
  if (!await verifyEd25519(authorization.publicKey, headerSignature, digest)) {
    return verificationFailure();
  }
  const root = inclusionRoot(header, kind);
  await verifyMerkleInclusion(canonicalLeaf, proof, root);
  return Object.freeze({
    level: kind === "state" ? "state-proven" : "batch-included",
    header,
    headerDigest: digest,
    root,
  });
}

function u32(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    return verificationFailure();
  }
  const result = new Uint8Array(4);
  new DataView(result.buffer).setUint32(0, value, false);
  return result;
}

function u16(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff) {
    return verificationFailure();
  }
  const result = new Uint8Array(2);
  new DataView(result.buffer).setUint16(0, value, false);
  return result;
}

function u64(value: bigint): Uint8Array {
  if (value < 0n || value > 0xffff_ffff_ffff_ffffn) {
    return verificationFailure();
  }
  const result = new Uint8Array(8);
  new DataView(result.buffer).setBigUint64(0, value, false);
  return result;
}

function attestationMessage(attestation: CheckpointAttestation): Uint8Array {
  return concatenate(
    u16(attestation.protocolVersion),
    u32(attestation.networkId),
    u64(attestation.paxeerChainId),
    exactBytes(attestation.settlementContract, 20),
    u64(attestation.epoch),
    exactBytes(attestation.checkpointId, 32),
    exactBytes(attestation.checkpointHash, 32),
    exactBytes(attestation.guarantorId, 32),
    u64(attestation.batchNumber),
    exactBytes(attestation.dataAvailabilityRoot, 32),
    new Uint8Array([
      attestation.replayed ? 1 : 0,
      attestation.dataPossessed ? 1 : 0,
      attestation.availabilityClassMask,
    ]),
    u64(attestation.attestedAtMs),
  );
}

export async function verifyCheckpoint(
  input: CheckpointVerificationInput,
  signatures: LocalSignatureVerifier,
): Promise<CheckpointVerification> {
  const { certificate } = input;
  if (!input.availabilityObtained || certificate.validityProof.length > 0xffff_ffff) {
    return verificationFailure();
  }
  const header = decodeBatchHeader(certificate.canonicalHeader);
  const checkpointId = await sha256(
    CHECKPOINT_DOMAIN,
    certificate.canonicalHeader,
    u32(certificate.validityProof.length),
    certificate.validityProof,
  );
  if (!equal(checkpointId, exactBytes(input.registeredCheckpointId, 32))) {
    return verificationFailure();
  }
  const expectedSettlementContract = exactBytes(input.expectedSettlementContract, 20);
  if (
    certificate.threshold <= 0
    || !Number.isSafeInteger(certificate.threshold)
    || input.expectedPaxeerChainId <= 0n
    || expectedSettlementContract.every((byte) => byte === 0)
  ) {
    return verificationFailure();
  }
  const seen = new Set<string>();
  let achieved = 0;
  for (const attestation of certificate.attestations) {
    const guarantorId = exactBytes(attestation.guarantorId, 32);
    const identity = Array.from(guarantorId, (byte) => byte.toString(16).padStart(2, "0")).join("");
    if (
      seen.has(identity)
      || attestation.protocolVersion !== header.protocolVersion
      || attestation.networkId !== header.networkId
      || attestation.epoch !== header.epoch
      || attestation.paxeerChainId !== input.expectedPaxeerChainId
      || !equal(exactBytes(attestation.settlementContract, 20), expectedSettlementContract)
      || !equal(attestation.checkpointId, checkpointId)
      || !equal(attestation.checkpointHash, checkpointId)
      || attestation.batchNumber !== header.batchNumber
      || !equal(attestation.dataAvailabilityRoot, header.dataAvailabilityRoot)
      || !attestation.replayed
      || !attestation.dataPossessed
      || attestation.availabilityClassMask !== ALL_AVAILABILITY_CLASSES
      || attestation.attestedAtMs <= 0n
      || exactBytes(attestation.signer, 20).every((byte) => byte === 0)
      || (attestation.signatureV !== 27 && attestation.signatureV !== 28)
    ) {
      return verificationFailure();
    }
    seen.add(identity);
    const member = input.bondedSet.find((candidate) => candidate.bonded && equal(candidate.guarantorId, guarantorId));
    if (member === undefined) {
      return verificationFailure();
    }
    const digest = await sha256(GUARANTOR_ATTESTATION_DOMAIN, attestationMessage(attestation));
    if (!await signatures.verifyRecoverableSecp256k1(
      exactBytes(member.publicKey, 33),
      exactBytes(attestation.signature, 64),
      attestation.signatureV,
      exactBytes(attestation.signer, 20),
      digest,
    )) {
      return verificationFailure();
    }
    achieved += 1;
  }
  if (achieved < certificate.threshold) {
    return verificationFailure();
  }
  const settlement = certificate.settlementReference;
  if (settlement !== undefined && (
    settlement.length === 0
    || input.registeredSettlementReference === undefined
    || !equal(settlement, input.registeredSettlementReference)
  )) {
    return verificationFailure();
  }
  return Object.freeze({
    level: settlement === undefined ? "checkpoint-finalised" : "settlement-anchored",
    checkpointId,
    achieved,
    required: certificate.threshold,
    header,
  });
}

interface DecodedReceipt {
  readonly receipt: ProtocolReceipt;
  readonly unsignedBytes: Uint8Array;
}

function allZero(value: Uint8Array): boolean {
  let aggregate = 0;
  for (const byte of value) {
    aggregate |= byte;
  }
  return aggregate === 0;
}

function decodeProgramReceiptOutcomeFrom(decoder: Decoder, protocolVersion: number): ProgramReceiptOutcome {
  const tag = decoder.u32();
  const encodingVersion = tag === PROGRAM_OUTCOME_V1 ? 1 : tag === PROGRAM_OUTCOME_V2 ? 2 : tag === PROGRAM_OUTCOME_V3 ? 3 : 0;
  if (encodingVersion === 0) {
    return verificationFailure();
  }
  const terminalKindValue = decoder.u8();
  if (terminalKindValue < 1 || terminalKindValue > 3) {
    return verificationFailure();
  }
  const terminalKind = terminalKindValue as 1 | 2 | 3;
  const resultCode = decoder.i32();
  const runtimeVersion = decoder.u16();
  const abiVersion = decoder.u16();
  const feeScheduleVersion = decoder.u32();
  const meteringScheduleVersion = encodingVersion === 3 ? decoder.u32() : 1;
  const cpuFuel = decoder.u64();
  const memoryBytes = decoder.u64();
  const storageReadBytes = decoder.u64();
  const storageWriteBytes = decoder.u64();
  const outputValues = decoder.u32();
  const outputBytes = decoder.u64();
  const occupancyByteBatches = encodingVersion >= 2 ? decoder.u128() : 0n;
  const occupancyFeeUnits = encodingVersion >= 2 ? decoder.u128() : 0n;
  const feeSchedulePrices: bigint[] = [];
  if (encodingVersion >= 2) {
    for (let index = 0; index < 7; index += 1) feeSchedulePrices.push(decoder.u64());
  } else {
    feeSchedulePrices.push(0n, 0n, 0n, 0n, 0n, 0n, 0n);
  }
  const occupancyAssetId = encodingVersion >= 2 ? decoder.bounded(32) : new Uint8Array(32);
  const occupancyEvidenceDigest = encodingVersion >= 2 ? decoder.bounded(32) : new Uint8Array(32);
  const occupancyTransferRoot = encodingVersion >= 2 ? decoder.bounded(32) : new Uint8Array(32);
  const feeUnits = decoder.u128();
  const callGraphRoot = decoder.bounded(32);
  const terminalPayloadRoot = decoder.bounded(32);
  const transferRoot = decoder.bounded(32);
  const occupancyZero = occupancyByteBatches === 0n
    && occupancyFeeUnits === 0n
    && allZero(occupancyAssetId)
    && allZero(occupancyEvidenceDigest)
    && allZero(occupancyTransferRoot);
  if (
    runtimeVersion === 0
    || abiVersion === 0
    || feeScheduleVersion === 0
    || meteringScheduleVersion !== 1
    || allZero(terminalPayloadRoot)
    || (terminalKind === 1 && resultCode !== 0)
    || (terminalKind !== 1 && (resultCode === 0 || resultCode <= -1000))
    || (terminalKind !== 1 && !allZero(transferRoot))
    || !((protocolVersion === 1 && (encodingVersion === 1 || encodingVersion === 3))
      || (protocolVersion === 2 && (encodingVersion === 2 || encodingVersion === 3)))
    || (encodingVersion === 1 && !occupancyZero)
    || (encodingVersion >= 2 && terminalKind !== 1 && !occupancyZero)
    || (encodingVersion === 2 && terminalKind === 1
      && (allZero(occupancyAssetId) || allZero(occupancyEvidenceDigest)))
    || (encodingVersion === 3 && allZero(occupancyAssetId) !== allZero(occupancyEvidenceDigest))
    || (protocolVersion === 1 && encodingVersion === 3 && !occupancyZero)
    || (protocolVersion === 2 && encodingVersion === 3 && terminalKind === 1
      && (allZero(occupancyAssetId) || allZero(occupancyEvidenceDigest)))
  ) {
    return verificationFailure();
  }
  return Object.freeze({
    encodingVersion,
    terminalKind,
    resultCode,
    runtimeVersion,
    abiVersion,
    feeScheduleVersion,
    meteringScheduleVersion,
    cpuFuel,
    memoryBytes,
    storageReadBytes,
    storageWriteBytes,
    outputValues,
    outputBytes,
    occupancyByteBatches,
    occupancyFeeUnits,
    feeSchedulePrices: Object.freeze(feeSchedulePrices),
    occupancyAssetId,
    occupancyEvidenceDigest,
    occupancyTransferRoot,
    feeUnits,
    callGraphRoot,
    terminalPayloadRoot,
    transferRoot,
  });
}

export function decodeProgramReceiptOutcome(canonicalOutcome: Uint8Array, protocolVersion: number): ProgramReceiptOutcome {
  if (canonicalOutcome.length === 0 || canonicalOutcome.length > MAX_MESSAGE_BYTES) {
    return verificationFailure();
  }
  const decoder = new Decoder(canonicalOutcome.slice());
  const outcome = decodeProgramReceiptOutcomeFrom(decoder, protocolVersion);
  decoder.finish();
  return outcome;
}

function decodeProtocolReceipt(canonicalReceipt: Uint8Array): DecodedReceipt {
  if (canonicalReceipt.length === 0 || canonicalReceipt.length > MAX_MESSAGE_BYTES) {
    return verificationFailure();
  }
  const decoder = new Decoder(canonicalReceipt);
  const envelopeVersion = decoder.u16();
  if ((envelopeVersion !== 1 && envelopeVersion !== 2) || decoder.u16() !== 0x5201) {
    return verificationFailure();
  }
  const protocolVersion = decoder.u16();
  if (protocolVersion !== envelopeVersion) {
    return verificationFailure();
  }
  const activityId = decoder.bounded(32);
  const globalSequence = decoder.u64();
  const previousStateRoot = decoder.bounded(32);
  const resultingStateRoot = decoder.bounded(32);
  const activityRoot = decoder.bounded(32);
  const resultCode = decoder.i32();
  const effectCount = decoder.u32();
  if (effectCount > MAX_EFFECTS) {
    return verificationFailure();
  }
  const effects: ReceiptEffect[] = [];
  for (let index = 0; index < effectCount; index += 1) {
    const moduleId = decoder.u16();
    const ordinal = decoder.u16();
    const eventType = decoder.u16();
    const kindValue = decoder.u8();
    if (kindValue < 1 || kindValue > 3) {
      return verificationFailure();
    }
    const monetaryValue = decoder.u8();
    if (monetaryValue > 1 || (monetaryValue === 1 && kindValue !== 2)) {
      return verificationFailure();
    }
    effects.push(Object.freeze({
      moduleId,
      ordinal,
      eventType,
      kind: kindValue as 1 | 2 | 3,
      monetary: monetaryValue === 1,
      transferSetRoot: decoder.bounded(32),
      body: decoder.boundedAtMost(MAX_EFFECT_BODY),
    }));
  }
  const feeCharged = decoder.u128();
  const batchId = decoder.bounded(32);
  const moduleId = decoder.u16();
  const moduleVersion = decoder.u32();
  const parameterVersion = decoder.u32();
  const operation = decoder.u8();
  const asset = decoder.bounded(32);
  const amount = decoder.u128();
  const from = decoder.bounded(32);
  const fromBalanceBefore = decoder.u128();
  const fromBalanceAfter = decoder.u128();
  const fromSequence = decoder.u64();
  const to = decoder.bounded(32);
  const toBalanceBefore = decoder.u128();
  const toBalanceAfter = decoder.u128();
  const transferSetRoot = decoder.bounded(32);
  const authorizationHash = decoder.bounded(32);
  const contextHash = decoder.bounded(32);
  const timestamp = decoder.u64();
  const programOutcome = decoder.remaining() > 69
    ? decodeProgramReceiptOutcomeFrom(decoder, protocolVersion)
    : undefined;
  if (programOutcome !== undefined && (
    moduleId !== 9
    || programOutcome.resultCode !== resultCode
    || (programOutcome.terminalKind === 1 && !equal(programOutcome.transferRoot, transferSetRoot))
    || (programOutcome.terminalKind !== 1 && !allZero(transferSetRoot))
  )) {
    return verificationFailure();
  }
  const signatureFlagOffset = decoder.position();
  if (decoder.u8() !== 1) {
    return verificationFailure();
  }
  const sequencerSignature = decoder.bounded(64);
  decoder.finish();
  return Object.freeze({
    receipt: Object.freeze({
      protocolVersion,
      activityId,
      globalSequence,
      previousStateRoot,
      resultingStateRoot,
      activityRoot,
      resultCode,
      effects: Object.freeze(effects),
      feeCharged,
      batchId,
      moduleId,
      moduleVersion,
      parameterVersion,
      operation,
      asset,
      amount,
      from,
      fromBalanceBefore,
      fromBalanceAfter,
      fromSequence,
      to,
      toBalanceBefore,
      toBalanceAfter,
      transferSetRoot,
      authorizationHash,
      contextHash,
      timestamp,
      ...(programOutcome === undefined ? {} : { programOutcome }),
      sequencerSignature,
    }),
    unsignedBytes: concatenate(canonicalReceipt.slice(0, signatureFlagOffset), new Uint8Array([0])),
  });
}

export async function verifyReceiptOutcome(
  canonicalReceipt: Uint8Array,
  authorized: AuthorizedReceiptBatch,
): Promise<ReceiptVerification> {
  const canonical = canonicalReceipt.slice();
  const authority = Object.freeze({
    batchId: exactBytes(authorized.batchId, 32).slice(),
    asset: exactBytes(authorized.asset, 32).slice(),
    previousStateRoot: exactBytes(authorized.previousStateRoot, 32).slice(),
    resultingStateRoot: exactBytes(authorized.resultingStateRoot, 32).slice(),
    sequencerPublicKey: exactBytes(authorized.sequencerPublicKey, 32).slice(),
  });
  const { receipt, unsignedBytes } = decodeProtocolReceipt(canonical);
  if (
    receipt.operation === 0
    || allZero(receipt.activityId)
    || allZero(receipt.asset)
    || !equal(receipt.batchId, authority.batchId)
    || !equal(receipt.asset, authority.asset)
    || !equal(receipt.previousStateRoot, authority.previousStateRoot)
    || !equal(receipt.resultingStateRoot, authority.resultingStateRoot)
  ) {
    return verificationFailure();
  }
  if (receipt.resultCode === 0) {
    if (
      receipt.fromBalanceBefore < receipt.amount
      || receipt.fromBalanceBefore - receipt.amount !== receipt.fromBalanceAfter
      || receipt.toBalanceBefore + receipt.amount > MAX_U128
      || receipt.toBalanceBefore + receipt.amount !== receipt.toBalanceAfter
    ) {
      return verificationFailure();
    }
  }
  const receiptDigest = await sha256(RECEIPT_DOMAIN, unsignedBytes);
  if (!await verifyEd25519(
    authority.sequencerPublicKey,
    receipt.sequencerSignature,
    receiptDigest,
  )) {
    return verificationFailure();
  }
  return Object.freeze({
    level: "sequencer-signed",
    receipt,
    canonicalBytes: canonical,
    receiptDigest,
  });
}

export async function verifyReceipt(
  canonicalReceipt: Uint8Array,
  authorized: AuthorizedReceiptBatch,
): Promise<ReceiptVerification> {
  const verified = await verifyReceiptOutcome(canonicalReceipt, authorized);
  if (verified.receipt.resultCode !== 0) {
    return verificationFailure();
  }
  return verified;
}
