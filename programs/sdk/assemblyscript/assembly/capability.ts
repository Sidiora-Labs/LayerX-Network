/**
 * Explicit capability grants and their canonical encoding.
 *
 * A program holds no ambient authority. Every effect it produces is checked
 * against a grant the invoking activity fixed before guest code began, and a
 * call to another program may only hand on a narrowing of what this program
 * already holds. Sets are kept in the runtime's authority-key order and refuse
 * duplicate keys, so the bytes handed to `program_call` are the same bytes the
 * host would have produced for the same grants, in every authoring language.
 */

import { AMOUNT_BYTES, MAX_CAPABILITIES, MAX_CAPABILITY_ENCODING_BYTES } from "./abi";
import { Amount } from "./amount";
import { IDENTIFIER_BYTES, compare, copy, isZero, writeU16BE } from "./bytes";
import {
  ERR_BUFFER_TOO_SMALL,
  ERR_CAPABILITY_BYTES,
  ERR_CAPABILITY_LIMIT,
  ERR_DUPLICATE_CAPABILITY,
  ERR_INVALID,
  ERR_RESERVED_IDENTIFIER,
  ERR_ZERO_AMOUNT,
  OK
} from "./error";
import { AccountId, AssetId, ProgramId, ReceiptDigest } from "./ids";

/** Authority to read this program's namespaced storage. */
export const CAPABILITY_STORAGE_READ: u8 = 1;
/** Authority to write and delete this program's namespaced storage. */
export const CAPABILITY_STORAGE_WRITE: u8 = 2;
/** Authority to emit events under this program's namespace. */
export const CAPABILITY_EMIT_EVENT: u8 = 3;
/** Authority to call one named program. */
export const CAPABILITY_CALL: u8 = 4;
/** Authority to request bounded 402LXP transfers of one asset to one account. */
export const CAPABILITY_TRANSFER_402: u8 = 5;
/** Authority to read the facts of one verified receipt. */
export const CAPABILITY_RECEIPT_READ: u8 = 6;
export const CAPABILITY_SHARED_STORAGE_READ: u8 = 7;
export const CAPABILITY_SHARED_STORAGE_WRITE: u8 = 8;

const COUNT_BYTES: i32 = 2;
const TAG_BYTES: i32 = 1;

function emptyIdentifier(): StaticArray<u8> {
  return new StaticArray<u8>(IDENTIFIER_BYTES);
}

function exactIdentifier(value: StaticArray<u8>): bool {
  return value.length == IDENTIFIER_BYTES;
}

function cloneIdentifier(value: StaticArray<u8>): StaticArray<u8> {
  const cloned = new StaticArray<u8>(IDENTIFIER_BYTES);
  copy(cloned, 0, value, 0, IDENTIFIER_BYTES);
  return cloned;
}

/**
 * One explicit authority granted by the invoking activity.
 *
 * The payload is stored the way the canonical encoding lays it out: a tag, the
 * identifier the grant names, and for a transfer grant the account it may credit
 * together with the exact integer ceiling.
 */
export class Capability {
  private readonly capabilityKind: u8;
  private readonly capabilityIdentifier: StaticArray<u8>;
  private readonly capabilityRecipient: StaticArray<u8>;
  private readonly capabilityMaximumAmount: Amount;

  private constructor(
    kind: u8,
    identifier: StaticArray<u8>,
    recipient: StaticArray<u8>,
    maximumAmount: Amount
  ) {
    this.capabilityKind = kind;
    this.capabilityIdentifier = cloneIdentifier(identifier);
    this.capabilityRecipient = cloneIdentifier(recipient);
    this.capabilityMaximumAmount = maximumAmount;
  }

  /** Grants read authority over this program's namespaced storage. */
  static storageRead(): Capability {
    return new Capability(CAPABILITY_STORAGE_READ, emptyIdentifier(), emptyIdentifier(), Amount.zero());
  }

  /** Grants write and delete authority over this program's namespaced storage. */
  static storageWrite(): Capability {
    return new Capability(CAPABILITY_STORAGE_WRITE, emptyIdentifier(), emptyIdentifier(), Amount.zero());
  }
  static sharedStorageRead(): Capability {
    return new Capability(CAPABILITY_SHARED_STORAGE_READ, emptyIdentifier(), emptyIdentifier(), Amount.zero());
  }
  static sharedStorageWrite(): Capability {
    return new Capability(CAPABILITY_SHARED_STORAGE_WRITE, emptyIdentifier(), emptyIdentifier(), Amount.zero());
  }

  /** Grants authority to emit events under this program's namespace. */
  static emitEvent(): Capability {
    return new Capability(CAPABILITY_EMIT_EVENT, emptyIdentifier(), emptyIdentifier(), Amount.zero());
  }

  /** Grants call authority over one named program. */
  static call(program: ProgramId): Capability {
    return new Capability(CAPABILITY_CALL, program.bytes, emptyIdentifier(), Amount.zero());
  }

  /** Grants bounded 402LXP transfer authority. */
  static transfer402(asset: AssetId, to: AccountId, maximumAmount: Amount): Capability {
    return new Capability(CAPABILITY_TRANSFER_402, asset.bytes, to.bytes, maximumAmount);
  }

  /** Grants read authority over the facts of one verified receipt. */
  static receiptRead(receiptDigest: ReceiptDigest): Capability {
    return new Capability(CAPABILITY_RECEIPT_READ, receiptDigest.bytes, emptyIdentifier(), Amount.zero());
  }

  /** Frozen capability tag. */
  get kind(): u8 {
    return this.capabilityKind;
  }

  /** Frozen primary authority key. */
  get identifier(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityIdentifier);
  }

  /** Frozen transfer destination, or the canonical zero payload. */
  get recipient(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityRecipient);
  }

  /** Frozen transfer ceiling. */
  get maximumAmount(): Amount {
    return this.capabilityMaximumAmount;
  }

  /** Program a call grant may enter. */
  get program(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityIdentifier);
  }

  /** Asset a transfer grant may move. */
  get asset(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityIdentifier);
  }

  /** Account a transfer grant may credit. */
  get to(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityRecipient);
  }

  /** Digest a receipt grant may read. */
  get receiptDigest(): StaticArray<u8> {
    return cloneIdentifier(this.capabilityIdentifier);
  }

  /** Refuses a grant the runtime's own law would reject. */
  validate(): i32 {
    if (
      this.capabilityKind == CAPABILITY_STORAGE_READ ||
      this.capabilityKind == CAPABILITY_STORAGE_WRITE ||
      this.capabilityKind == CAPABILITY_SHARED_STORAGE_READ ||
      this.capabilityKind == CAPABILITY_SHARED_STORAGE_WRITE ||
      this.capabilityKind == CAPABILITY_EMIT_EVENT
    ) {
      return OK;
    }
    if (this.capabilityKind == CAPABILITY_CALL || this.capabilityKind == CAPABILITY_RECEIPT_READ) {
      return !exactIdentifier(this.capabilityIdentifier) || isZero(this.capabilityIdentifier)
        ? ERR_RESERVED_IDENTIFIER
        : OK;
    }
    if (this.capabilityKind == CAPABILITY_TRANSFER_402) {
      if (
        !exactIdentifier(this.capabilityIdentifier) ||
        !exactIdentifier(this.capabilityRecipient) ||
        isZero(this.capabilityIdentifier) ||
        isZero(this.capabilityRecipient)
      ) return ERR_RESERVED_IDENTIFIER;
      if (this.capabilityMaximumAmount.isZero()) return ERR_ZERO_AMOUNT;
      return OK;
    }
    return ERR_INVALID;
  }

  /** Bytes this grant occupies in the canonical capability-list encoding. */
  encodedLength(): i32 {
    if (
      this.capabilityKind == CAPABILITY_STORAGE_READ ||
      this.capabilityKind == CAPABILITY_STORAGE_WRITE ||
      this.capabilityKind == CAPABILITY_SHARED_STORAGE_READ ||
      this.capabilityKind == CAPABILITY_SHARED_STORAGE_WRITE ||
      this.capabilityKind == CAPABILITY_EMIT_EVENT
    ) {
      return TAG_BYTES;
    }
    if (this.capabilityKind == CAPABILITY_CALL || this.capabilityKind == CAPABILITY_RECEIPT_READ) {
      return TAG_BYTES + IDENTIFIER_BYTES;
    }
    if (this.capabilityKind == CAPABILITY_TRANSFER_402) {
      return TAG_BYTES + IDENTIFIER_BYTES + IDENTIFIER_BYTES + AMOUNT_BYTES;
    }
    return 0;
  }

  /** Orders two grants by the runtime's own authority key. */
  compareAuthority(right: Capability): i32 {
    if (this.capabilityKind != right.capabilityKind) {
      return this.capabilityKind < right.capabilityKind ? -1 : 1;
    }
    if (this.capabilityKind == CAPABILITY_CALL || this.capabilityKind == CAPABILITY_RECEIPT_READ) {
      return compare(
        this.capabilityIdentifier,
        0,
        right.capabilityIdentifier,
        0,
        IDENTIFIER_BYTES
      );
    }
    if (this.capabilityKind == CAPABILITY_TRANSFER_402) {
      const order = compare(
        this.capabilityIdentifier,
        0,
        right.capabilityIdentifier,
        0,
        IDENTIFIER_BYTES
      );
      if (order != 0) return order;
      return compare(
        this.capabilityRecipient,
        0,
        right.capabilityRecipient,
        0,
        IDENTIFIER_BYTES
      );
    }
    return 0;
  }
}

/**
 * Closed set of explicit capabilities holding at most the declared number of
 * grants. Grants are stored in the runtime's authority-key order and duplicate
 * keys are refused, so no ambiguous limit can reach the host.
 */
export class CapabilitySet {
  private declaredCapacity: i32;
  private configurationStatus: i32;
  private grants: Array<Capability>;

  constructor(capacity: i32) {
    this.grants = new Array<Capability>();
    this.configurationStatus = capacity < 0 || capacity > MAX_CAPABILITIES
      ? ERR_CAPABILITY_LIMIT
      : OK;
    this.declaredCapacity = this.configurationStatus == OK ? capacity : 0;
  }

  /** Reports whether the declared capacity was canonical. */
  get status(): i32 {
    return this.configurationStatus;
  }

  /** Declared upper bound on the number of grants this set may hold. */
  get capacity(): i32 {
    return this.declaredCapacity;
  }

  /** Number of grants held. */
  get length(): i32 {
    return this.grants.length;
  }

  /** Borrows one held grant in authority-key order. */
  grant(index: i32): Capability {
    return this.grants[index];
  }

  /** Reports whether the set already holds the given authority key. */
  holds(grant: Capability): bool {
    for (let index = 0; index < this.grants.length; index++) {
      if (this.grants[index].compareAuthority(grant) == 0) return true;
    }
    return false;
  }

  /** Adds one validated grant in authority-key order. */
  insert(grant: Capability): i32 {
    if (this.configurationStatus != OK) return this.configurationStatus;
    const valid = grant.validate();
    if (valid != OK) return valid;
    if (this.grants.length >= this.declaredCapacity) return ERR_CAPABILITY_LIMIT;
    if (this.grants.length >= MAX_CAPABILITIES) return ERR_CAPABILITY_LIMIT;
    let position = 0;
    while (position < this.grants.length) {
      const order = this.grants[position].compareAuthority(grant);
      if (order == 0) return ERR_DUPLICATE_CAPABILITY;
      if (order > 0) break;
      position += 1;
    }
    this.grants.push(grant);
    let index = this.grants.length - 1;
    while (index > position) {
      this.grants[index] = this.grants[index - 1];
      index -= 1;
    }
    this.grants[position] = grant;
    return OK;
  }

  /** Exact length of this set's canonical encoding. */
  encodedLength(): i32 {
    let total = COUNT_BYTES;
    for (let index = 0; index < this.grants.length; index++) {
      total += this.grants[index].encodedLength();
    }
    return total;
  }

  /**
   * Encodes this set into the frozen deterministic capability-list format
   * `program_call` consumes, returning the number of bytes written or a
   * negative refusal.
   */
  encode(out: StaticArray<u8>): i32 {
    if (this.configurationStatus != OK) return this.configurationStatus;
    const required = this.encodedLength();
    if (required > MAX_CAPABILITY_ENCODING_BYTES) return ERR_CAPABILITY_BYTES;
    if (required > out.length) return ERR_BUFFER_TOO_SMALL;
    writeU16BE(out, 0, <u16>this.grants.length);
    let cursor = COUNT_BYTES;
    for (let index = 0; index < this.grants.length; index++) {
      const grant = this.grants[index];
      out[cursor] = grant.kind;
      cursor += TAG_BYTES;
      if (grant.kind == CAPABILITY_CALL || grant.kind == CAPABILITY_RECEIPT_READ) {
        copy(out, cursor, grant.identifier, 0, IDENTIFIER_BYTES);
        cursor += IDENTIFIER_BYTES;
      } else if (grant.kind == CAPABILITY_TRANSFER_402) {
        copy(out, cursor, grant.identifier, 0, IDENTIFIER_BYTES);
        cursor += IDENTIFIER_BYTES;
        copy(out, cursor, grant.recipient, 0, IDENTIFIER_BYTES);
        cursor += IDENTIFIER_BYTES;
        grant.maximumAmount.writeBigEndian(out, cursor);
        cursor += AMOUNT_BYTES;
      }
    }
    return cursor;
  }

  /** Encodes this set into a freshly sized array with an explicit status. */
  toBytes(): CapabilityEncoding {
    if (this.configurationStatus != OK) {
      return new CapabilityEncoding(this.configurationStatus, new StaticArray<u8>(0));
    }
    const required = this.encodedLength();
    if (required > MAX_CAPABILITY_ENCODING_BYTES) {
      return new CapabilityEncoding(ERR_CAPABILITY_BYTES, new StaticArray<u8>(0));
    }
    const out = new StaticArray<u8>(required);
    const written = this.encode(out);
    if (written < 0) return new CapabilityEncoding(written, new StaticArray<u8>(0));
    return new CapabilityEncoding(OK, out);
  }
}

/** Explicit result of allocating and encoding a capability set. */
export class CapabilityEncoding {
  readonly status: i32;
  readonly bytes: StaticArray<u8>;

  constructor(status: i32, bytes: StaticArray<u8>) {
    this.status = status;
    this.bytes = bytes;
  }

  ok(): bool { return this.status == OK; }
}
