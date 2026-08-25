/**
 * Exact, nonzero protocol identifiers.
 *
 * Storage is private and every public byte view is a fresh exact-width copy,
 * so callers cannot mutate an identifier after validation or hand a host call
 * a short array whose pointer is advertised as thirty-two bytes.
 */

import { IDENTIFIER_BYTES, identifierFromWords, isZero, slice } from "./bytes";

function canonical(source: StaticArray<u8>, offset: i32): StaticArray<u8> | null {
  if (offset < 0 || offset > source.length - IDENTIFIER_BYTES) return null;
  const value = slice(source, offset, IDENTIFIER_BYTES);
  return isZero(value) ? null : value;
}

function clone(value: StaticArray<u8>): StaticArray<u8> {
  return slice(value, 0, IDENTIFIER_BYTES);
}

/** Stable identifier of a deployed program. */
export class ProgramId {
  private readonly value: StaticArray<u8>;

  private constructor(value: StaticArray<u8>) { this.value = value; }

  static fromWords(word0: u64, word1: u64, word2: u64, word3: u64): ProgramId | null {
    return ProgramId.fromBytes(identifierFromWords(word0, word1, word2, word3), 0);
  }

  static fromBytes(source: StaticArray<u8>, offset: i32): ProgramId | null {
    const value = canonical(source, offset);
    return value === null ? null : new ProgramId(value);
  }

  get bytes(): StaticArray<u8> { return clone(this.value); }
  isReserved(): bool { return false; }
}

/** Stable identifier of a protocol asset. */
export class AssetId {
  private readonly value: StaticArray<u8>;

  private constructor(value: StaticArray<u8>) { this.value = value; }

  static fromWords(word0: u64, word1: u64, word2: u64, word3: u64): AssetId | null {
    return AssetId.fromBytes(identifierFromWords(word0, word1, word2, word3), 0);
  }

  static fromBytes(source: StaticArray<u8>, offset: i32): AssetId | null {
    const value = canonical(source, offset);
    return value === null ? null : new AssetId(value);
  }

  get bytes(): StaticArray<u8> { return clone(this.value); }
  isReserved(): bool { return false; }
}

/** Stable identifier of an account a 402LXP transfer may credit. */
export class AccountId {
  private readonly value: StaticArray<u8>;

  private constructor(value: StaticArray<u8>) { this.value = value; }

  static fromWords(word0: u64, word1: u64, word2: u64, word3: u64): AccountId | null {
    return AccountId.fromBytes(identifierFromWords(word0, word1, word2, word3), 0);
  }

  static fromBytes(source: StaticArray<u8>, offset: i32): AccountId | null {
    const value = canonical(source, offset);
    return value === null ? null : new AccountId(value);
  }

  get bytes(): StaticArray<u8> { return clone(this.value); }
  isReserved(): bool { return false; }
}

/** Digest naming one verified protocol receipt. */
export class ReceiptDigest {
  private readonly value: StaticArray<u8>;

  private constructor(value: StaticArray<u8>) { this.value = value; }

  static fromWords(word0: u64, word1: u64, word2: u64, word3: u64): ReceiptDigest | null {
    return ReceiptDigest.fromBytes(identifierFromWords(word0, word1, word2, word3), 0);
  }

  static fromBytes(source: StaticArray<u8>, offset: i32): ReceiptDigest | null {
    const value = canonical(source, offset);
    return value === null ? null : new ReceiptDigest(value);
  }

  get bytes(): StaticArray<u8> { return clone(this.value); }
  isReserved(): bool { return false; }
}
