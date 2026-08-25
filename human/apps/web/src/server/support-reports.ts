import { createHash, randomUUID, timingSafeEqual } from "node:crypto";
import { link, mkdir, open, readFile, readdir, stat, unlink } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";

import {
  parseStoredSupportReport,
  supportReportTrace,
  type StoredSupportReport,
  type SupportReportRequest,
} from "../states/report-schema.ts";

const REPORT_FILE = /^[a-f0-9]{64}\.json$/u;
const PRINCIPAL_DIRECTORY = /^[a-f0-9]{64}$/u;
const DEFAULT_RETENTION = 1_000;
const MAX_RETENTION = 10_000;

export class SupportReportConfigurationError extends Error {}
export class SupportReportConflictError extends Error {}

function nodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

function reportId(traceId: string): string {
  return createHash("sha256").update(traceId, "utf8").digest("hex");
}

function principalDirectory(principalScope: string): string {
  if (!/^[A-Za-z0-9_-]{16,128}$/u.test(principalScope)) {
    throw new SupportReportConfigurationError("Support report principal scope is invalid");
  }
  return createHash("sha256").update(principalScope, "utf8").digest("hex");
}

function retentionFromEnvironment(): number {
  const configured = process.env.LAYERX_SUPPORT_REPORT_RETENTION;
  if (configured === undefined || configured.trim() === "") {
    return DEFAULT_RETENTION;
  }
  const parsed = Number(configured);
  if (!Number.isSafeInteger(parsed) || parsed < 1 || parsed > MAX_RETENTION) {
    throw new SupportReportConfigurationError("Support report retention is invalid");
  }
  return parsed;
}

function storageDirectoryFromEnvironment(): string {
  const configured = process.env.LAYERX_SUPPORT_REPORT_DIR?.trim();
  if (configured === undefined || configured === "" || !isAbsolute(configured)) {
    throw new SupportReportConfigurationError("Support report storage directory is not configured");
  }
  const directory = resolve(configured);
  if (directory === "/") {
    throw new SupportReportConfigurationError("Support report storage directory is too broad");
  }
  return directory;
}

async function syncDirectory(directory: string): Promise<void> {
  const handle = await open(directory, "r");
  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

function sameStoredReport(stored: StoredSupportReport, report: SupportReportRequest): boolean {
  return stored.traceId === report.traceId
    && stored.machineCode === report.machineCode
    && stored.route === report.route
    && stored.shell === report.shell;
}

export class SupportReportRepository {
  readonly directory: string;
  readonly retention: number;

  constructor(directory: string, retention: number) {
    if (!isAbsolute(directory) || resolve(directory) === "/") {
      throw new SupportReportConfigurationError("Support report storage directory is invalid");
    }
    if (!Number.isSafeInteger(retention) || retention < 1 || retention > MAX_RETENTION) {
      throw new SupportReportConfigurationError("Support report retention is invalid");
    }
    this.directory = directory;
    this.retention = retention;
  }

  async save(principalScope: string, report: SupportReportRequest): Promise<Readonly<{ record: StoredSupportReport; created: boolean }>> {
    const principal = join(this.directory, principalDirectory(principalScope));
    await mkdir(principal, { recursive: true, mode: 0o700 });
    const identifier = reportId(report.traceId);
    const record = Object.freeze({
      ...report,
      reportId: identifier,
      receivedAt: new Date().toISOString(),
    });
    const destination = join(principal, `${identifier}.json`);
    const temporary = join(principal, `.pending-${identifier}-${randomUUID()}`);
    const handle = await open(temporary, "wx", 0o600);
    try {
      await handle.writeFile(`${JSON.stringify(record)}\n`, "utf8");
      await handle.sync();
    } finally {
      await handle.close();
    }

    try {
      await link(temporary, destination);
      await syncDirectory(principal);
    } catch (error) {
      if (!nodeError(error) || error.code !== "EEXIST") {
        throw error;
      }
      const existing = await this.findByTrace(report.traceId, principalScope);
      if (existing === undefined || !sameStoredReport(existing, report)) {
        throw new SupportReportConflictError("Trace already belongs to a different report");
      }
      await this.#enforceRetention(principal);
      return Object.freeze({ record: existing, created: false });
    } finally {
      await unlink(temporary).catch((error: unknown) => {
        if (!nodeError(error) || error.code !== "ENOENT") {
          throw error;
        }
      });
    }

    await this.#enforceRetention(principal, `${identifier}.json`);
    return Object.freeze({ record, created: true });
  }

  async findByTrace(traceId: string, principalScope?: string): Promise<StoredSupportReport | undefined> {
    const normalized = supportReportTrace(traceId);
    const filename = `${reportId(normalized)}.json`;
    const locations = principalScope === undefined
      ? await this.#reportLocations(filename)
      : [join(this.directory, principalDirectory(principalScope), filename)];
    let found: StoredSupportReport | undefined;
    for (const location of locations) {
      try {
        const report = parseStoredSupportReport(JSON.parse(await readFile(location, "utf8")));
        if (found !== undefined) throw new SupportReportConflictError("Trace belongs to multiple principals");
        found = report;
      } catch (error) {
        if (nodeError(error) && error.code === "ENOENT") continue;
        throw error;
      }
    }
    return found;
  }

  async #reportLocations(filename: string): Promise<string[]> {
    try {
      const entries = await readdir(this.directory, { withFileTypes: true });
      return entries
        .filter((entry) => entry.isDirectory() && PRINCIPAL_DIRECTORY.test(entry.name))
        .map((entry) => join(this.directory, entry.name, filename));
    } catch (error) {
      if (nodeError(error) && error.code === "ENOENT") {
        return [];
      }
      throw error;
    }
  }

  async #enforceRetention(principal: string, protectedFile?: string): Promise<void> {
    const entries = (await readdir(principal, { withFileTypes: true }))
      .filter((entry) => entry.isFile() && REPORT_FILE.test(entry.name));
    if (entries.length <= this.retention) {
      return;
    }
    const aged = await Promise.all(entries.map(async (entry) => ({
      name: entry.name,
      modifiedAt: (await stat(join(principal, entry.name))).mtimeMs,
    })));
    aged.sort((left, right) => left.modifiedAt - right.modifiedAt || left.name.localeCompare(right.name));
    const expiredReports = aged
      .filter((entry) => entry.name !== protectedFile)
      .slice(0, aged.length - this.retention);
    for (const expired of expiredReports) {
      await unlink(join(principal, expired.name)).catch((error: unknown) => {
        if (!nodeError(error) || error.code !== "ENOENT") {
          throw error;
        }
      });
    }
    await syncDirectory(principal);
  }
}

export function supportReportRepositoryFromEnvironment(): SupportReportRepository {
  return new SupportReportRepository(storageDirectoryFromEnvironment(), retentionFromEnvironment());
}

export function supportRetrievalAuthorized(request: Request): boolean {
  const credential = process.env.LAYERX_SUPPORT_REPORT_BEARER_TOKEN?.trim();
  if (credential === undefined || credential.length < 32) {
    throw new SupportReportConfigurationError("Support report retrieval credential is not configured");
  }
  const supplied = request.headers.get("authorization") ?? "";
  const expectedDigest = createHash("sha256").update(`Bearer ${credential}`, "utf8").digest();
  const suppliedDigest = createHash("sha256").update(supplied, "utf8").digest();
  return timingSafeEqual(expectedDigest, suppliedDigest);
}
