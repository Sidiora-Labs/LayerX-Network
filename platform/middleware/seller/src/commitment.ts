import {
  PlatformSdkError,
  verifyBatchInclusion,
  verifyCheckpoint,
  type CheckpointVerificationInput,
  type LocalSignatureVerifier,
  type MerkleProof,
  type ReceiptVerification,
  type SequencerAuthorization,
} from "@sidiora/layerx-sdk";

export type PaymentCommitment = "executed" | "batched" | "finalised";

export interface PaymentCommitmentEvidence {
  readonly networkId: number;
  readonly canonicalHeader: Uint8Array;
  readonly headerSignature: Uint8Array;
  readonly authorization: SequencerAuthorization;
  readonly proof: MerkleProof;
  readonly checkpoint?: {
    readonly input: CheckpointVerificationInput;
    readonly signatures: LocalSignatureVerifier;
    readonly requiredGuarantors: number;
  };
}

export interface PaymentCommitmentResolver {
  resolve(canonicalReceipt: Uint8Array, network: string, commitment: PaymentCommitment): Promise<PaymentCommitmentEvidence>;
}

function failure(): never {
  throw new PlatformSdkError({ code: "verification-failure", retry: "never" });
}

export function paymentCommitment(extra: unknown): PaymentCommitment {
  if (extra === undefined || extra === null || typeof extra !== "object" || Array.isArray(extra)) return "executed";
  const layerx = (extra as Record<string, unknown>)["layerx"];
  if (layerx === undefined) return "executed";
  if (layerx === null || typeof layerx !== "object" || Array.isArray(layerx)) return failure();
  const commitment = (layerx as Record<string, unknown>)["commitment"];
  if (commitment !== "executed" && commitment !== "batched" && commitment !== "finalised") return failure();
  return commitment;
}

function layerXTerms(extra: unknown): Record<string, unknown> | undefined {
  if (extra === undefined || extra === null || typeof extra !== "object" || Array.isArray(extra)) return undefined;
  const layerx = (extra as Record<string, unknown>)["layerx"];
  if (layerx === undefined) return undefined;
  if (layerx === null || typeof layerx !== "object" || Array.isArray(layerx)) return failure();
  return layerx as Record<string, unknown>;
}

export function paymentPayer(extra: unknown, required = false): string | undefined {
  const payer = layerXTerms(extra)?.["payer"];
  if (payer === undefined && !required) return undefined;
  if (typeof payer !== "string" || !/^[0-9a-f]{64}$/u.test(payer) || /^0+$/u.test(payer)) return failure();
  return payer;
}

export function paymentPurpose(extra: unknown, required = false): string | undefined {
  const purpose = layerXTerms(extra)?.["purposeHash"];
  if (purpose === undefined && !required) return undefined;
  if (typeof purpose !== "string" || !/^[0-9a-f]{64}$/u.test(purpose) || /^0+$/u.test(purpose)) return failure();
  return purpose;
}

function equal(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}

export async function verifyPaymentCommitment(
  verified: ReceiptVerification,
  sequencerPublicKey: Uint8Array,
  network: string,
  commitment: PaymentCommitment,
  resolver?: PaymentCommitmentResolver,
): Promise<void> {
  if (commitment === "executed") return;
  if (commitment !== "batched" && commitment !== "finalised") return failure();
  if (resolver === undefined) return failure();
  const evidence = await resolver.resolve(verified.canonicalBytes.slice(), network, commitment);
  await verifyPaymentCommitmentEvidence(verified, sequencerPublicKey, commitment, evidence);
}

export async function verifyPaymentCommitmentEvidence(
  verified: ReceiptVerification,
  sequencerPublicKey: Uint8Array,
  commitment: PaymentCommitment,
  evidence: PaymentCommitmentEvidence,
): Promise<void> {
  if (commitment === "executed") return;
  if (commitment !== "batched" && commitment !== "finalised") return failure();
  evidence = {
    networkId: evidence.networkId,
    canonicalHeader: evidence.canonicalHeader.slice(),
    headerSignature: evidence.headerSignature.slice(),
    authorization: structuredClone(evidence.authorization),
    proof: structuredClone(evidence.proof),
    ...(evidence.checkpoint === undefined ? {} : { checkpoint: {
      input: structuredClone(evidence.checkpoint.input),
      signatures: evidence.checkpoint.signatures,
      requiredGuarantors: evidence.checkpoint.requiredGuarantors,
    } }),
  };
  sequencerPublicKey = sequencerPublicKey.slice();
  const version = verified.receipt.protocolVersion;
  if ((version !== 2 && version !== 3)
    || !Number.isSafeInteger(evidence.networkId) || evidence.networkId <= 0
    || evidence.networkId > 0xffff_ffff
    || !equal(evidence.authorization.publicKey, sequencerPublicKey)) return failure();
  const inclusion = await verifyBatchInclusion(
    "receipt", verified.canonicalBytes, evidence.proof, evidence.canonicalHeader,
    evidence.headerSignature, evidence.authorization, { protocolVersion: version },
  );
  if (inclusion.header.networkId !== evidence.networkId
    || verified.receipt.globalSequence < inclusion.header.firstSequence
    || verified.receipt.globalSequence > inclusion.header.lastSequence) return failure();
  if (commitment === "batched") return;
  const checkpoint = evidence.checkpoint;
  if (checkpoint === undefined || !Number.isSafeInteger(checkpoint.requiredGuarantors)
    || checkpoint.requiredGuarantors <= 0
    || checkpoint.input.certificate.threshold !== checkpoint.requiredGuarantors
    || !equal(checkpoint.input.certificate.canonicalHeader, evidence.canonicalHeader)) return failure();
  await verifyCheckpoint(checkpoint.input, checkpoint.signatures, { protocolVersion: version });
}
