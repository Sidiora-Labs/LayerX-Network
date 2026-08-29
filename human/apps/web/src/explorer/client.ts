import "server-only";

import {
  decodeAccountActivity,
  decodeBatch,
  decodeCheckpoint,
  decodePage,
  decodeProgram,
  decodeReceipt,
  decodeRecord,
  decodeVerificationReport,
  validExplorerCoordinate,
  validExplorerIdentifier,
  type AccountActivityRecord,
  type BatchRecord,
  type CheckpointRecord,
  type EvidenceVerificationReport,
  type ExplorerPage,
  type ExplorerRecord,
  type ProgramRecord,
  type ReceiptRecord,
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

export async function checkpointPage(
  before?: string,
  limit = "25",
): Promise<ExplorerPage<CheckpointRecord>> {
  if ((before !== undefined && !validExplorerCoordinate(before)) || !validExplorerCoordinate(limit)) {
    throw new TypeError("Invalid checkpoint page coordinate");
  }
  return decodePage(
    await get("/v1/explorer/checkpoints", { ...(before === undefined ? {} : { before }), limit }),
    decodeCheckpoint,
    "checkpoint page",
  );
}

export async function batchPage(
  before?: string,
  limit = "25",
): Promise<ExplorerPage<BatchRecord>> {
  if ((before !== undefined && !validExplorerCoordinate(before)) || !validExplorerCoordinate(limit)) {
    throw new TypeError("Invalid batch page coordinate");
  }
  return decodePage(
    await get("/v1/explorer/batches", { ...(before === undefined ? {} : { before }), limit }),
    decodeBatch,
    "batch page",
  );
}

export async function checkpointRecord(
  identifier: string,
): Promise<ExplorerRecord<CheckpointRecord>> {
  if (!validExplorerIdentifier(identifier)) {
    throw new TypeError("Invalid checkpoint identifier");
  }
  return decodeRecord(
    await get(`/v1/explorer/checkpoints/${encodeURIComponent(identifier.toLowerCase())}`),
    decodeCheckpoint,
    "checkpoint record",
  );
}

export async function batchRecord(batch: string): Promise<ExplorerRecord<BatchRecord>> {
  if (!validExplorerCoordinate(batch)) {
    throw new TypeError("Invalid batch number");
  }
  return decodeRecord(await get(`/v1/explorer/batches/${encodeURIComponent(batch)}`), decodeBatch, "batch record");
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
