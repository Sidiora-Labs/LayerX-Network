import type { AuthorizedReceiptBatch } from "@sidiora/layerx-sdk";
import type {
  LayerXReceiptEvidence,
  PaymentPayload,
  PaymentRequired,
  PaymentRequirements,
} from "@sidiora/layerx-seller-middleware";

const RECEIPT_DOMAIN = new TextEncoder().encode("LXP/v1/receipt\0");
const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");

const RECEIPT_MAGIC = 0x5201;
const RECEIPT_MAGIC_PREFIX = 1;
const PROTOCOL_VERSION = 1;

/**
 * A signing sequencer used by the conformance suite. It builds byte-identical
 * canonical receipts and authorised-batch descriptors that the shipped SDK
 * verifier accepts, using a real Ed25519 key pair. It is a genuine receipt
 * producer exercising the real wire format, not a verification double.
 */
export class ConformanceSequencer {
  readonly #privateKey: CryptoKey;

  public readonly publicKey: Uint8Array;

  private constructor(privateKey: CryptoKey, publicKey: Uint8Array) {
    this.#privateKey = privateKey;
    this.publicKey = publicKey;
  }

  public static async generate(): Promise<ConformanceSequencer> {
    const pair = (await globalThis.crypto.subtle.generateKey(
      { name: "Ed25519" },
      true,
      ["sign", "verify"],
    )) as CryptoKeyPair;
    const raw = new Uint8Array(await globalThis.crypto.subtle.exportKey("raw", pair.publicKey));
    return new ConformanceSequencer(pair.privateKey, raw);
  }

  public async sign(digest: Uint8Array): Promise<Uint8Array> {
    const signature = await globalThis.crypto.subtle.sign(
      { name: "Ed25519" },
      this.#privateKey,
      arrayBuffer(digest),
    );
    return new Uint8Array(signature);
  }
}

export interface ReceiptFacts {
  readonly asset: Uint8Array;
  readonly payTo: Uint8Array;
  readonly amount: bigint;
  readonly from: Uint8Array;
  readonly batchId: Uint8Array;
  readonly previousStateRoot: Uint8Array;
  readonly resultingStateRoot: Uint8Array;
}

export interface SignedReceipt {
  readonly canonicalReceipt: Uint8Array;
  readonly authorizedBatch: AuthorizedReceiptBatch;
  readonly receiptDigest: string;
  readonly evidence: LayerXReceiptEvidence;
}

/**
 * Encodes and signs a canonical protocol receipt for the given facts. The byte
 * layout mirrors `decodeProtocolReceipt` in the SDK verifier exactly, so the
 * shipped `verifyReceipt` accepts the result under the returned authorised
 * batch. Balances satisfy the verifier's monetary invariants for a successful
 * transfer.
 */
export async function buildSignedReceipt(
  sequencer: ConformanceSequencer,
  facts: ReceiptFacts,
): Promise<SignedReceipt> {
  const fromBalanceBefore = facts.amount + 1_000_000n;
  const fromBalanceAfter = fromBalanceBefore - facts.amount;
  const toBalanceBefore = 0n;
  const toBalanceAfter = toBalanceBefore + facts.amount;

  const writer = new ByteWriter();
  writer.u16(RECEIPT_MAGIC_PREFIX);
  writer.u16(RECEIPT_MAGIC);
  writer.u16(PROTOCOL_VERSION);
  writer.bounded(fixedBytes(0x11));
  writer.u64(7n);
  writer.bounded(facts.previousStateRoot);
  writer.bounded(facts.resultingStateRoot);
  writer.bounded(fixedBytes(0x22));
  writer.u32(0);
  writer.u32(0);
  writer.u128(0n);
  writer.bounded(facts.batchId);
  writer.u16(1);
  writer.u32(1);
  writer.u32(1);
  writer.u8(1);
  writer.bounded(facts.asset);
  writer.u128(facts.amount);
  writer.bounded(facts.from);
  writer.u128(fromBalanceBefore);
  writer.u128(fromBalanceAfter);
  writer.u64(3n);
  writer.bounded(facts.payTo);
  writer.u128(toBalanceBefore);
  writer.u128(toBalanceAfter);
  writer.bounded(fixedBytes(0x33));
  writer.bounded(fixedBytes(0x44));
  writer.bounded(fixedBytes(0x55));
  writer.u64(1_726_000_000_000n);

  const prefix = writer.finish();
  const unsigned = concatenate(prefix, new Uint8Array([0]));
  const digest = await sha256(RECEIPT_DOMAIN, unsigned);
  const signature = await sequencer.sign(digest);
  if (signature.length !== 64) {
    throw new Error("conformance sequencer produced a non-Ed25519 signature");
  }

  const signed = new ByteWriter();
  signed.raw(prefix);
  signed.u8(1);
  signed.bounded64(signature);
  const canonicalReceipt = signed.finish();

  const authorizedBatch: AuthorizedReceiptBatch = {
    batchId: facts.batchId,
    asset: facts.asset,
    previousStateRoot: facts.previousStateRoot,
    resultingStateRoot: facts.resultingStateRoot,
    sequencerPublicKey: sequencer.publicKey,
  };
  const receiptDigest = toHex(await sha256(MERKLE_LEAF_DOMAIN, canonicalReceipt));
  const evidence: LayerXReceiptEvidence = {
    receipt: encodeBase64(canonicalReceipt),
    receiptDigest,
    verificationLevel: "sequencer-signed",
  };
  return { canonicalReceipt, authorizedBatch, receiptDigest, evidence };
}

export interface OfferFixture {
  readonly requirements: PaymentRequirements;
  readonly paymentRequired: PaymentRequired;
}

export function offerFixture(payTo: Uint8Array, asset: Uint8Array, amount: bigint): OfferFixture {
  const requirements: PaymentRequirements = {
    scheme: "exact",
    network: "layerx:testnet",
    amount: amount.toString(),
    asset: toHex(asset),
    payTo: toHex(payTo),
    maxTimeoutSeconds: 120,
  };
  const paymentRequired: PaymentRequired = {
    x402Version: 2,
    resource: {
      url: "https://paid.example/paid",
      description: "conformance resource",
      mimeType: "application/json",
    },
    accepts: [requirements],
    extensions: {},
  };
  return { requirements, paymentRequired };
}

export function buyerPayload(offer: OfferFixture, receipt: SignedReceipt, key: string): PaymentPayload {
  return {
    x402Version: 2,
    resource: offer.paymentRequired.resource,
    payload: {
      receipt: receipt.evidence.receipt,
      receiptDigest: receipt.evidence.receiptDigest,
      verificationLevel: "sequencer-signed",
      idempotencyKey: key,
    },
    accepted: offer.requirements,
    extensions: {},
  };
}

export function fixedBytes(seed: number): Uint8Array {
  return bytes32(new Uint8Array(0), seed);
}

class ByteWriter {
  #chunks: Uint8Array[] = [];

  public u8(value: number): void {
    this.#chunks.push(new Uint8Array([value & 0xff]));
  }

  public u16(value: number): void {
    this.#chunks.push(bigEndian(BigInt(value), 2));
  }

  public u32(value: number): void {
    this.#chunks.push(bigEndian(BigInt(value), 4));
  }

  public u64(value: bigint): void {
    this.#chunks.push(bigEndian(value, 8));
  }

  public u128(value: bigint): void {
    this.#chunks.push(bigEndian(value, 16));
  }

  public bounded(value: Uint8Array): void {
    if (value.length !== 32) {
      throw new Error(`bounded field must be 32 bytes, got ${value.length}`);
    }
    this.u32(32);
    this.#chunks.push(value);
  }

  public bounded64(value: Uint8Array): void {
    if (value.length !== 64) {
      throw new Error(`signature field must be 64 bytes, got ${value.length}`);
    }
    this.u32(64);
    this.#chunks.push(value);
  }

  public raw(value: Uint8Array): void {
    this.#chunks.push(value);
  }

  public finish(): Uint8Array {
    return concatenate(...this.#chunks);
  }
}

function bytes32(source: Uint8Array, fill: number): Uint8Array {
  if (source.length === 32) {
    return source;
  }
  return new Uint8Array(32).fill(fill & 0xff);
}

function bigEndian(value: bigint, length: number): Uint8Array {
  if (value < 0n) {
    throw new Error("cannot encode a negative integer");
  }
  const result = new Uint8Array(length);
  let remaining = value;
  for (let index = length - 1; index >= 0; index -= 1) {
    result[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  if (remaining !== 0n) {
    throw new Error(`integer ${value} overflows ${length} bytes`);
  }
  return result;
}

function arrayBuffer(value: Uint8Array): ArrayBuffer {
  const copy = new ArrayBuffer(value.length);
  new Uint8Array(copy).set(value);
  return copy;
}

function concatenate(...values: readonly Uint8Array[]): Uint8Array {
  const result = new Uint8Array(values.reduce((total, value) => total + value.length, 0));
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

export function encodeBase64(value: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < value.length; offset += 0x8000) {
    binary += String.fromCharCode(...value.subarray(offset, Math.min(offset + 0x8000, value.length)));
  }
  return btoa(binary);
}

export function toHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
