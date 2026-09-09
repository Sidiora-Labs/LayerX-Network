import {
  PlatformSdkError,
  grantPaymentTerms,
  paymentCommitment,
  paymentPayer,
  verifyBatchInclusion,
  verifyCheckpoint,
  type CheckpointVerificationInput,
  type LocalSignatureVerifier,
  type MerkleProof,
  type PaymentCommitment,
  type ReceiptVerification,
  type SequencerAuthorization,
} from "@sidiora/layerx-sdk";

export { grantPaymentTerms, paymentCommitment, paymentPayer };
export type { PaymentCommitment };

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
