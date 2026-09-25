import "server-only";

import { explorerLinkPath, parseExplorerBaseUrl, type ExplorerLinkTarget } from "./links";
import {
  decodeAccountActivity,
  decodeNameResolution,
  decodePage,
  decodeProgram,
  decodeReceipt,
  decodeRecord,
  decodeUnifiedAccount,
  decodeVerificationReport,
  parseAccountIdentifier,
  validExplorerCoordinate,
  validExplorerIdentifier,
  validExplorerName,
  type AccountActivityRecord,
  type EvidenceVerificationReport,
  type ExplorerPage,
  type ExplorerRecord,
  type NameResolutionRecord,
  type ProgramRecord,
  type ReceiptRecord,
  type UnifiedAccountRecord,
} from "./model";

const FETCH_TIMEOUT_MS = 8_000;

export class ExplorerUnavailableError extends Error {
  constructor() {
    super("The public explorer service is unavailable");
    this.name = "ExplorerUnavailableError";
  }
}

function programExplorerOrigin(): Readonly<{ origin: URL; bearer: string }> {
  const configured = process.env.LAYERX_EXPLORER_PROGRAM_API_ORIGIN;
  const bearer = process.env.LAYERX_EXPLORER_PROGRAM_BEARER_TOKEN;
  if (configured === undefined || bearer === undefined || bearer.length < 32) {
    throw new ExplorerUnavailableError();
  }
  let origin: URL;
  try {
    origin = new URL(configured);
  } catch {
    throw new ExplorerUnavailableError();
  }
  const loopback = origin.hostname === "127.0.0.1" || origin.hostname === "localhost";
  if (!loopback || origin.protocol !== "http:" || origin.pathname !== "/") {
    throw new ExplorerUnavailableError();
  }
  return Object.freeze({ origin, bearer });
}

function explorerOrigin(): URL {
  const configured = process.env.LAYERX_EXPLORER_API_ORIGIN;
  if (configured === undefined) {
    throw new ExplorerUnavailableError();
  }
  let origin: URL;
  try {
    origin = new URL(configured);
  } catch {
    throw new ExplorerUnavailableError();
  }
  const loopback = origin.hostname === "127.0.0.1" || origin.hostname === "localhost";
  if ((origin.protocol !== "https:" && !(loopback && origin.protocol === "http:")) || origin.pathname !== "/") {
    throw new ExplorerUnavailableError();
  }
  return origin;
}

/// PAXEER_X_EXPLORER_BASE_URL names the public base URL of the Paxeer X Network explorer, the
/// one surface that renders anchors, receipts, transactions and addresses. It is the only place
/// the explorer's host is configured: no host is written into this repository, and an unset or
/// malformed value fails closed with the unavailable state rather than guessing an origin.
function explorerBaseUrl(): URL {
  const base = parseExplorerBaseUrl(process.env.PAXEER_X_EXPLORER_BASE_URL);
  if (base === undefined) {
    throw new ExplorerUnavailableError();
  }
  return base;
}

export function explorerLink(target: ExplorerLinkTarget): string {
  return new URL(explorerLinkPath(target), explorerBaseUrl()).toString();
}

async function get(path: string, query?: Readonly<Record<string, string>>): Promise<unknown> {
  const url = new URL(path, explorerOrigin());
  for (const [name, value] of Object.entries(query ?? {})) {
    url.searchParams.set(name, value);
  }
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { Accept: "application/json" },
      cache: "force-cache",
      next: { revalidate: 60 },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch {
    throw new ExplorerUnavailableError();
  }
  if (!response.ok) {
    if (response.status === 404) {
      try {
        return await response.json();
      } catch {
        throw new ExplorerUnavailableError();
      }
    }
    throw new ExplorerUnavailableError();
  }
  try {
    return await response.json();
  } catch {
    throw new ExplorerUnavailableError();
  }
}

export async function receiptRecord(
  identifier: string,
): Promise<ExplorerRecord<ReceiptRecord>> {
  if (!validExplorerIdentifier(identifier)) {
    throw new TypeError("Invalid receipt identifier");
  }
  return decodeRecord(
    await get(`/v1/explorer/receipts/${encodeURIComponent(identifier.toLowerCase())}`),
    decodeReceipt,
    "receipt record",
  );
}

export async function accountActivityPage(
  identifier: string,
  before?: string,
  limit = "25",
): Promise<ExplorerPage<AccountActivityRecord>> {
  if (
    !validExplorerIdentifier(identifier)
    || (before !== undefined && !validExplorerCoordinate(before))
    || !validExplorerCoordinate(limit)
  ) {
    throw new TypeError("Invalid account activity query");
  }
  return decodePage(
    await get(`/v1/explorer/accounts/${encodeURIComponent(identifier.toLowerCase())}`, {
      ...(before === undefined ? {} : { before }),
      limit,
    }),
    decodeAccountActivity,
    "account activity page",
  );
}

export async function programRecord(identifier: string): Promise<ProgramRecord | undefined> {
  if (!validExplorerIdentifier(identifier)) {
    throw new TypeError("Invalid program identifier");
  }
  const { origin, bearer } = programExplorerOrigin();
  const url = new URL(`/v1/programs/${encodeURIComponent(identifier.toLowerCase())}`, origin);
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { Accept: "application/json", Authorization: `Bearer ${bearer}` },
      cache: "no-store",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch {
    throw new ExplorerUnavailableError();
  }
  if (response.status === 404) {
    return undefined;
  }
  if (!response.ok) {
    throw new ExplorerUnavailableError();
  }
  try {
    return decodeProgram(await response.json());
  } catch (error) {
    if (error instanceof TypeError) {
      throw error;
    }
    throw new ExplorerUnavailableError();
  }
}

export async function unifiedAccount(
  identifier: string,
  beforeBlock?: string,
): Promise<UnifiedAccountRecord | undefined> {
  const account = parseAccountIdentifier(identifier);
  if (account === undefined || (beforeBlock !== undefined && !validExplorerCoordinate(beforeBlock))) {
    throw new TypeError("Invalid unified account query");
  }
  const { origin, bearer } = programExplorerOrigin();
  const url = new URL(`/v1/accounts/${encodeURIComponent(account.canonical)}/unified`, origin);
  if (beforeBlock !== undefined) {
    url.searchParams.set("before_block", beforeBlock);
  }
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { Accept: "application/json", Authorization: `Bearer ${bearer}` },
      cache: "no-store",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch {
    throw new ExplorerUnavailableError();
  }
  if (response.status === 404) {
    return undefined;
  }
  if (!response.ok) {
    throw new ExplorerUnavailableError();
  }
  try {
    return decodeUnifiedAccount(await response.json());
  } catch (error) {
    if (error instanceof TypeError) {
      throw error;
    }
    throw new ExplorerUnavailableError();
  }
}

function namingProgram(): string {
  const configured = process.env.LAYERX_EXPLORER_NAMING_PROGRAM;
  if (configured === undefined || !validExplorerIdentifier(configured)) {
    throw new ExplorerUnavailableError();
  }
  return configured.toLowerCase();
}

export async function resolveName(name: string): Promise<NameResolutionRecord | undefined> {
  if (!validExplorerName(name)) {
    throw new TypeError("Invalid name");
  }
  const { origin, bearer } = programExplorerOrigin();
  const url = new URL(`/v1/programs/${namingProgram()}/reads/resolve`, origin);
  url.searchParams.set("name", name);
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { Accept: "application/json", Authorization: `Bearer ${bearer}` },
      cache: "no-store",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch {
    throw new ExplorerUnavailableError();
  }
  if (response.status === 404) {
    return undefined;
  }
  if (!response.ok) {
    throw new ExplorerUnavailableError();
  }
  try {
    return decodeNameResolution(await response.json());
  } catch (error) {
    if (error instanceof TypeError) {
      throw error;
    }
    throw new ExplorerUnavailableError();
  }
}

export async function verifyEvidenceUpstream(
  request: Readonly<Record<string, unknown>>,
): Promise<EvidenceVerificationReport> {
  const url = new URL("/v1/explorer/verify", explorerOrigin());
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: JSON.stringify(request),
      cache: "no-store",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch {
    throw new ExplorerUnavailableError();
  }
  if (!response.ok) {
    throw new TypeError("Evidence did not verify");
  }
  try {
    return decodeVerificationReport(await response.json());
  } catch (error) {
    if (error instanceof TypeError) {
      throw error;
    }
    throw new ExplorerUnavailableError();
  }
}
