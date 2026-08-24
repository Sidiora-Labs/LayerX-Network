export type MirrorLag =
  | Readonly<{ kind: "known"; batches: bigint }>
  | Readonly<{ kind: "unknown" }>;

export type MirrorProvenance = "canonical" | "reorged";

export interface MirrorTarget {
  readonly kind: "ethereum" | "solana";
  readonly networkIdentity: string;
  readonly contractOrProgram: string;
  readonly codeIdentity: string;
  readonly publisher: string;
}

export interface MirrorObservation {
  readonly sourceId: string;
  readonly target: MirrorTarget;
  readonly commitment: Uint8Array;
  readonly batchNumber: bigint;
  readonly canonicalPosition: string;
  readonly provenance: MirrorProvenance;
  readonly latestBatch?: bigint;
  readonly latestCheckpointBatch?: bigint;
  readonly lag: MirrorLag;
}

export interface MirrorCandidate {
  readonly source: number;
  readonly commitment: Uint8Array;
}

export type MirrorReadPolicy =
  | Readonly<{ kind: "exact"; candidate: MirrorCandidate }>
  | Readonly<{ kind: "ordered-preference"; candidates: readonly MirrorCandidate[] }>
  | Readonly<{
      kind: "agreement";
      candidates: readonly MirrorCandidate[];
      minimum: number;
    }>;
