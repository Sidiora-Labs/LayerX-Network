/**
 * The 402LXP transfer binding.
 *
 * A program cannot write a balance. It can only request an authenticated
 * transfer the kernel applies atomically after the whole execution succeeds, and
 * only inside the ceiling its capability grant fixed. Amounts are exact protocol
 * integers, refused when zero, and identifiers are refused when reserved.
 */

import { IDENTIFIER_BYTES, pointer } from "./bytes";
import { Amount } from "./amount";
import { ERR_RESERVED_IDENTIFIER, ERR_ZERO_AMOUNT, OK } from "./error";
import { transfer402 as hostTransfer402 } from "./host";
import { AccountId, AssetId } from "./ids";

/** Requests one authenticated 402LXP transfer. */
export function transfer402(asset: AssetId, to: AccountId, amount: Amount): i32 {
  const assetBytes = asset.bytes;
  const recipientBytes = to.bytes;
  if (amount.isZero()) return ERR_ZERO_AMOUNT;
  if (
    assetBytes.length != IDENTIFIER_BYTES ||
    recipientBytes.length != IDENTIFIER_BYTES ||
    asset.isReserved() ||
    to.isReserved()
  ) return ERR_RESERVED_IDENTIFIER;
  return hostTransfer402(
    amount.highWord(),
    amount.lowWord(),
    pointer(assetBytes),
    IDENTIFIER_BYTES,
    pointer(recipientBytes),
    IDENTIFIER_BYTES
  );
}

/** One authenticated 402LXP transfer the kernel will apply. */
export class Payment {
  asset: AssetId;
  to: AccountId;
  amount: Amount;

  constructor(asset: AssetId, to: AccountId, amount: Amount) {
    this.asset = asset;
    this.to = to;
    this.amount = amount;
  }

  /** Refuses a payment the runtime's monetary law would reject. */
  validate(): i32 {
    if (this.amount.isZero()) return ERR_ZERO_AMOUNT;
    if (this.asset.isReserved() || this.to.isReserved()) return ERR_RESERVED_IDENTIFIER;
    return OK;
  }

  /** Requests this payment. */
  send(): i32 {
    return transfer402(this.asset, this.to, this.amount);
  }
}
