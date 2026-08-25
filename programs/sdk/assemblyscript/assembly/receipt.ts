/**
 * Verified receipt facts.
 *
 * A program never sees kernel state. It reads only the facts the core has
 * already verified for a digest the invoking activity named in an explicit
 * grant, decoded from the frozen one hundred and sixteen byte view. The decoded
 * digest is checked against the requested one, so evidence for another receipt
 * cannot be mistaken for evidence for this one.
 */

import { DIGEST_BYTES, IDENTIFIER_BYTES, RECEIPT_ENCODING_BYTES } from "./abi";
import { Amount } from "./amount";
import { equal, pointer, readI32BE, slice } from "./bytes";
import { ERR_EVIDENCE, ERR_RECEIPT_ENCODING, ERR_RESERVED_IDENTIFIER, OK } from "./error";
import { receiptRead } from "./host";
import { ReceiptDigest } from "./ids";

const DIGEST_OFFSET: i32 = 0;
const RESULT_CODE_OFFSET: i32 = 32;
const ASSET_OFFSET: i32 = 36;
const AMOUNT_OFFSET: i32 = 68;
const STATE_ROOT_OFFSET: i32 = 84;

/** Verified receipt facts exposed without raw kernel state. */
export class Receipt {
  digest: StaticArray<u8>;
  resultCode: i32;
  asset: StaticArray<u8>;
  amount: Amount;
  stateRoot: StaticArray<u8>;

  constructor(
    digest: StaticArray<u8>,
    resultCode: i32,
    asset: StaticArray<u8>,
    amount: Amount,
    stateRoot: StaticArray<u8>
  ) {
    this.digest = digest;
    this.resultCode = resultCode;
    this.asset = asset;
    this.amount = amount;
    this.stateRoot = stateRoot;
  }

  /** An empty view, returned alongside a refusal so no fact is invented. */
  static empty(): Receipt {
    return new Receipt(
      new StaticArray<u8>(DIGEST_BYTES),
      0,
      new StaticArray<u8>(IDENTIFIER_BYTES),
      Amount.zero(),
      new StaticArray<u8>(DIGEST_BYTES)
    );
  }

  /** Reports whether the receipt records a successful settlement. */
  settled(): bool {
    return this.resultCode == 0;
  }
}

/** The outcome of one receipt read. */
export class ReceiptRead {
  status: i32;
  receipt: Receipt;

  constructor(status: i32, receipt: Receipt) {
    this.status = status;
    this.receipt = receipt;
  }

  /** Reports whether the host produced verified facts. */
  ok(): bool {
    return this.status == OK;
  }
}

/** Decodes the frozen receipt view the host writes. */
export function decodeReceipt(encoded: StaticArray<u8>): ReceiptRead {
  if (encoded.length != RECEIPT_ENCODING_BYTES) {
    return new ReceiptRead(ERR_RECEIPT_ENCODING, Receipt.empty());
  }
  const receipt = new Receipt(
    slice(encoded, DIGEST_OFFSET, DIGEST_BYTES),
    readI32BE(encoded, RESULT_CODE_OFFSET),
    slice(encoded, ASSET_OFFSET, IDENTIFIER_BYTES),
    Amount.fromBigEndian(encoded, AMOUNT_OFFSET),
    slice(encoded, STATE_ROOT_OFFSET, DIGEST_BYTES)
  );
  return new ReceiptRead(OK, receipt);
}

/** Reads the verified facts of one receipt named by an explicit grant. */
export function readReceipt(receiptDigest: ReceiptDigest): ReceiptRead {
  const digestBytes = receiptDigest.bytes;
  if (digestBytes.length != DIGEST_BYTES || receiptDigest.isReserved()) {
    return new ReceiptRead(ERR_RESERVED_IDENTIFIER, Receipt.empty());
  }
  const encoded = new StaticArray<u8>(RECEIPT_ENCODING_BYTES);
  const outcome = receiptRead(
    pointer(digestBytes),
    DIGEST_BYTES,
    pointer(encoded),
    RECEIPT_ENCODING_BYTES
  );
  if (outcome < 0) return new ReceiptRead(outcome, Receipt.empty());
  if (outcome != RECEIPT_ENCODING_BYTES) {
    return new ReceiptRead(ERR_RECEIPT_ENCODING, Receipt.empty());
  }
  const decoded = decodeReceipt(encoded);
  if (!decoded.ok()) return decoded;
  if (!equal(decoded.receipt.digest, 0, digestBytes, 0, DIGEST_BYTES)) {
    return new ReceiptRead(ERR_EVIDENCE, Receipt.empty());
  }
  return decoded;
}
