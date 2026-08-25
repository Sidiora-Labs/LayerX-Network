/**
 * The protocol monetary integer.
 *
 * An amount is an exact unsigned one hundred and twenty-eight bit integer held
 * as two sixty-four bit halves, the same representation the kernel and the Go
 * SDK use. There is no floating-point constructor, no floating-point conversion
 * and no overloaded arithmetic operator that could silently wrap, so a program
 * cannot express money the deterministic runtime is unable to reproduce. Every
 * arithmetic operation is checked and a refusal yields no value at all.
 */

import { AMOUNT_BYTES, readU64BE, writeU64BE } from "./bytes";
import { ERR_OVERFLOW, ERR_UNDERFLOW, OK } from "./error";

/** An exact unsigned one hundred and twenty-eight bit protocol amount. */
export class Amount {
  private readonly hi: u64;
  private readonly lo: u64;

  private constructor(hi: u64, lo: u64) {
    this.hi = hi;
    this.lo = lo;
  }

  /** Zero amount. */
  static zero(): Amount {
    return new Amount(0, 0);
  }

  /** The largest amount the protocol width holds. */
  static max(): Amount {
    return new Amount(u64.MAX_VALUE, u64.MAX_VALUE);
  }

  /** Widens an exact unsigned integer into an amount. */
  static fromU64(value: u64): Amount {
    return new Amount(0, value);
  }

  /** Builds an amount from the network-order word pair the ABI carries. */
  static fromParts(hi: u64, lo: u64): Amount {
    return new Amount(hi, lo);
  }

  /** Decodes the canonical big-endian sixteen-byte representation. */
  static fromBigEndian(source: StaticArray<u8>, offset: i32): Amount {
    return new Amount(readU64BE(source, offset), readU64BE(source, offset + 8));
  }

  /** Writes the canonical big-endian bytes into a caller-owned array. */
  writeBigEndian(out: StaticArray<u8>, offset: i32): void {
    writeU64BE(out, offset, this.hi);
    writeU64BE(out, offset + 8, this.lo);
  }

  /** Returns the canonical big-endian bytes. */
  toBigEndian(): StaticArray<u8> {
    const out = new StaticArray<u8>(AMOUNT_BYTES);
    this.writeBigEndian(out, 0);
    return out;
  }

  /** Reports whether this is the zero the monetary law refuses. */
  isZero(): bool {
    return this.hi == 0 && this.lo == 0;
  }

  /** Orders two amounts exactly. */
  compare(right: Amount): i32 {
    if (this.hi != right.hi) return this.hi < right.hi ? -1 : 1;
    if (this.lo != right.lo) return this.lo < right.lo ? -1 : 1;
    return 0;
  }

  /** Reports whether two amounts are the same exact integer. */
  equals(right: Amount): bool {
    return this.hi == right.hi && this.lo == right.lo;
  }

  /** Returns the high half as the signed word `transfer_402` carries. */
  highWord(): i64 {
    return <i64>this.hi;
  }

  /** Returns the low half as the signed word `transfer_402` carries. */
  lowWord(): i64 {
    return <i64>this.lo;
  }

  /** Adds without wrapping, refusing a sum past the protocol width. */
  add(right: Amount): AmountResult {
    const low = this.lo + right.lo;
    const carry: u64 = low < this.lo ? 1 : 0;
    if (this.hi > u64.MAX_VALUE - right.hi) {
      return new AmountResult(ERR_OVERFLOW, Amount.zero());
    }
    let high = this.hi + right.hi;
    if (high > u64.MAX_VALUE - carry) {
      return new AmountResult(ERR_OVERFLOW, Amount.zero());
    }
    high = high + carry;
    return new AmountResult(OK, new Amount(high, low));
  }

  /** Subtracts without wrapping, refusing a difference below zero. */
  sub(right: Amount): AmountResult {
    if (this.compare(right) < 0) {
      return new AmountResult(ERR_UNDERFLOW, Amount.zero());
    }
    const low = this.lo - right.lo;
    const borrow: u64 = this.lo < right.lo ? 1 : 0;
    const high = this.hi - right.hi - borrow;
    return new AmountResult(OK, new Amount(high, low));
  }
}

/** The outcome of one checked monetary operation. */
export class AmountResult {
  readonly status: i32;
  readonly value: Amount;

  constructor(status: i32, value: Amount) {
    this.status = status;
    this.value = value;
  }

  /** Reports whether the operation stayed inside the protocol width. */
  ok(): bool {
    return this.status == OK;
  }
}
