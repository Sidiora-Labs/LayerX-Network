import { decodeReceive, encodeReceive, type Receive } from "./receive.js";

export interface GrantOffer {
  readonly scheme: "metered" | "subscription";
  readonly asset: string;
  readonly amount: string;
  readonly payTo: string;
  readonly extra: { readonly layerx: {
    readonly commitment: "executed" | "batched" | "finalised";
    readonly purposeHash: string;
    readonly payer: string;
    readonly windowSeconds?: string;
  } };
}

export function validateGrantDraw(wire: Uint8Array, offer: GrantOffer, idempotencyKey: string, networkId: number, now: bigint): Receive {
  const receive = decodeReceive(wire);
  const grant = receive.payer_grant;
  const terms = offer.extra.layerx;
  const integer = (value: string, bits: number) => typeof value === "string" && /^(0|[1-9][0-9]{0,38})$/u.test(value) && BigInt(value) < 1n << BigInt(bits);
  if (!integer(offer.amount, 128) || BigInt(offer.amount) === 0n
    || !["executed", "batched", "finalised"].includes(terms.commitment)
    || !/^[0-9a-f]{64}$/u.test(terms.purposeHash) || /^0+$/u.test(terms.purposeHash)
    || !/^[0-9a-f]{64}$/u.test(terms.payer) || /^0+$/u.test(terms.payer)
    || receive.asset !== offer.asset || receive.to !== offer.payTo || receive.amount !== offer.amount
    || receive.idempotency_key !== idempotencyKey || receive.grant_id !== grant.grant_id
    || receive.from !== grant.from || receive.from !== terms.payer
    || receive.to !== grant.recipient || receive.asset !== grant.asset
    || grant.purpose_hash !== terms.purposeHash || BigInt(receive.amount) > BigInt(grant.per_draw_maximum)
    || BigInt(receive.amount) > BigInt(grant.allowance) || now < 0n || now >= BigInt(grant.expiration)
    || receive.receiver_authorization.network_id !== networkId
    || receive.receiver_authorization.controller !== receive.to
    || receive.receiver_authorization.signed_context_hash !== receive.context_hash
    || grant.has_reference || grant.reference_hash !== "00".repeat(32)) throw new Error("invalid-grant-draw");
  if (offer.scheme === "metered") {
    if (grant.recurring || grant.window_length !== "0" || terms.windowSeconds !== undefined) throw new Error("invalid-metered-grant");
  } else if (offer.scheme === "subscription") {
    if (!grant.recurring || !integer(terms.windowSeconds!, 64) || terms.windowSeconds === "0" || terms.windowSeconds !== grant.window_length) throw new Error("invalid-subscription-grant");
  } else throw new Error("unsupported-grant-scheme");
  encodeReceive(receive);
  return receive;
}
