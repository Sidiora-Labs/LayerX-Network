import { decodeBatchHeader, type CheckpointAttestation, type CheckpointVerificationInput, type LocalSignatureVerifier } from "@sidiora/layerx-sdk";
import { rpcHex, rpcObject } from "./rpc.js";
import type { PaymentCommitmentEvidence } from "./commitment.js";

export interface RpcCheckpointAuthority {
  readonly canonicalContext: Uint8Array;
  readonly verification: Omit<CheckpointVerificationInput, "certificate">;
  readonly requiredGuarantors: number;
  readonly signatures: LocalSignatureVerifier;
}

class Reader {
  private offset = 0;
  public constructor(private readonly bytes: Uint8Array) {}
  public take(size: number): Uint8Array {
    if (!Number.isSafeInteger(size) || size < 0 || this.offset + size > this.bytes.length) throw new Error("invalid-checkpoint-wire");
    const value = this.bytes.slice(this.offset, this.offset + size); this.offset += size; return value;
  }
  public integer(size: number): bigint { return this.take(size).reduce((n, b) => (n << 8n) | BigInt(b), 0n); }
  public number(size: number): number { return Number(this.integer(size)); }
  public boolean(): boolean { const n = this.number(1); if (n > 1) throw new Error("invalid-checkpoint-boolean"); return n === 1; }
  public sized(width: number, maximum: number): Uint8Array {
    const size = this.number(width); if (size > maximum) throw new Error("checkpoint-bound"); return this.take(size);
  }
  public finish(): void { if (this.offset !== this.bytes.length) throw new Error("checkpoint-trailing-bytes"); }
}

const equal = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((v, i) => v === b[i]);

export function rpcCheckpointEvidence(result: Record<string, unknown>, batch: PaymentCommitmentEvidence,
  authority: RpcCheckpointAuthority): PaymentCommitmentEvidence {
  const wire = rpcObject(result["checkpoint_evidence"]);
  if (!Number.isSafeInteger(authority.requiredGuarantors) || authority.requiredGuarantors <= 0 || authority.requiredGuarantors > 32
    || authority.canonicalContext.length === 0 || authority.canonicalContext.length > 131072
    || !equal(rpcHex(wire["context"]), authority.canonicalContext)
    || !equal(rpcHex(wire["checkpoint_id"], 32), authority.verification.registeredCheckpointId)) throw new Error("checkpoint-authority-mismatch");
  const canonicalHeader = rpcHex(wire["canonical_header"]);
  const header = decodeBatchHeader(canonicalHeader);
  if (!equal(canonicalHeader, batch.canonicalHeader) || header.networkId !== batch.networkId) throw new Error("checkpoint-header-mismatch");
  const reader = new Reader(rpcHex(wire["checkpoint"]));
  if (reader.number(2) !== 1 || !equal(reader.sized(4, 4096), canonicalHeader)) throw new Error("checkpoint-header-mismatch");
  const validityProof = reader.sized(4, 1048576);
  const count = reader.number(1);
  if (count === 0 || count > 32) throw new Error("checkpoint-attestation-count");
  const attestations: CheckpointAttestation[] = [];
  for (let i = 0; i < count; i++) {
    const attestation: CheckpointAttestation = {
      protocolVersion: reader.number(2), networkId: reader.number(4), paxeerChainId: reader.integer(8), settlementContract: reader.take(20),
      epoch: reader.integer(8), checkpointId: reader.take(32), checkpointHash: reader.take(32), guarantorId: reader.take(32),
      batchNumber: reader.integer(8), dataAvailabilityRoot: reader.take(32), replayed: reader.boolean(), dataPossessed: reader.boolean(),
      availabilityClassMask: reader.number(1), attestedAtMs: reader.integer(8), signer: reader.take(20), signature: reader.take(64), signatureV: reader.number(1),
    };
    const previous = attestations.at(-1);
    if (previous && Array.from(previous.guarantorId).map(v => v.toString(16).padStart(2, "0")).join("") >= Array.from(attestation.guarantorId).map(v => v.toString(16).padStart(2, "0")).join("")) throw new Error("checkpoint-guarantor-order");
    attestations.push(attestation);
  }
  const threshold = reader.number(1);
  const settlementReference = reader.sized(2, 1024);
  reader.finish();
  if (threshold !== authority.requiredGuarantors || threshold > count || settlementReference.length !== 110) throw new Error("checkpoint-threshold-or-settlement");
  return { ...batch, checkpoint: {
    input: { ...structuredClone(authority.verification), certificate: { canonicalHeader, validityProof, attestations, threshold, settlementReference } },
    signatures: authority.signatures, requiredGuarantors: authority.requiredGuarantors,
  } };
}
