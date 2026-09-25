import { validExplorerIdentifier } from "./model.ts";

/// The explorer surfaces the control plane links into. The Paxeer X Network explorer renders
/// every anchor - the batch it seals and the checkpoint it commits - on one anchor surface, so
/// the anchor, batch and checkpoint targets resolve to that surface; the receipt, transaction
/// and address targets carry the identifier their surface takes.
export const EXPLORER_LINK_KINDS = [
  "anchor",
  "batch",
  "checkpoint",
  "receipt",
  "transaction",
  "address",
] as const;

export type ExplorerLinkKind = (typeof EXPLORER_LINK_KINDS)[number];

export type ExplorerLinkTarget =
  | Readonly<{ kind: "anchor" }>
  | Readonly<{ kind: "batch" }>
  | Readonly<{ kind: "checkpoint" }>
  | Readonly<{ kind: "receipt"; receiptId: string }>
  | Readonly<{ kind: "transaction"; transactionHash: string }>
  | Readonly<{ kind: "address"; address: string }>;

export const EXPLORER_ANCHOR_PATH = "/paxeer-x/anchors";

const TRANSACTION_HASH_GRAMMAR = /^0x[0-9a-fA-F]{64}$/u;
const ADDRESS_GRAMMAR = /^0x[0-9a-fA-F]{40}$/u;

export function explorerLinkPath(target: ExplorerLinkTarget): string {
  switch (target.kind) {
    case "anchor":
    case "batch":
    case "checkpoint":
      return EXPLORER_ANCHOR_PATH;
    case "receipt":
      if (!validExplorerIdentifier(target.receiptId)) {
        throw new TypeError("Invalid receipt identifier");
      }
      return `/paxeer-x/receipts/${target.receiptId.toLowerCase()}`;
    case "transaction":
      if (!TRANSACTION_HASH_GRAMMAR.test(target.transactionHash)) {
        throw new TypeError("Invalid transaction hash");
      }
      return `/tx/${target.transactionHash.toLowerCase()}`;
    case "address":
      if (!ADDRESS_GRAMMAR.test(target.address)) {
        throw new TypeError("Invalid address");
      }
      return `/address/${target.address.toLowerCase()}`;
  }
}

/// The explorer's public base URL, as the control plane reads it from its environment. A base URL
/// is accepted only when it is an origin on its own: HTTPS, or HTTP for a loopback host in local
/// development, carrying no credentials, no path, no query and no fragment. Anything else, and an
/// unset variable, yields undefined so the caller fails closed instead of guessing an origin.
export function parseExplorerBaseUrl(configured: string | undefined): URL | undefined {
  if (configured === undefined) {
    return undefined;
  }
  let base: URL;
  try {
    base = new URL(configured);
  } catch {
    return undefined;
  }
  const loopback = base.hostname === "127.0.0.1" || base.hostname === "localhost";
  if (
    (base.protocol !== "https:" && !(loopback && base.protocol === "http:"))
    || base.username !== ""
    || base.password !== ""
    || base.pathname !== "/"
    || base.search !== ""
    || base.hash !== ""
  ) {
    return undefined;
  }
  return base;
}

/// The control plane's own explorer routes, in the order they are offered. Every entry names a
/// route the control plane still serves; the anchor listings it used to serve are linked to the
/// explorer from the overview instead.
export const EXPLORER_NAVIGATION = [
  Object.freeze({ href: "/explorer", copyKey: "explorer.navigation.overview" }),
  Object.freeze({ href: "/explorer/verify", copyKey: "explorer.navigation.verify" }),
] as const;
