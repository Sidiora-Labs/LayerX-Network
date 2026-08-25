import {
  parseSavedReportReceipt,
  parseSupportReportRequest,
  type ReportShell,
  type SavedReportReceipt,
  type SupportReportRequest,
} from "./report-schema.ts";
import { csrfTokenFromCookie } from "../auth/session.ts";

export interface ErrorReportDraft {
  readonly consented: boolean;
  readonly traceId: string;
  readonly machineCode: string;
  readonly route: string;
  readonly shell: ReportShell;
}

export type ErrorReportReceipt = SavedReportReceipt | Readonly<{
  traceId: string;
  status: "pending";
}>;

export interface ErrorReporter {
  submit(draft: ErrorReportDraft): Promise<ErrorReportReceipt>;
}

const PENDING_REPORTS_KEY = "layerx.human.pending-support-reports.v1";
const MAX_PENDING_REPORTS = 50;

class ReportServerError extends Error {
  readonly status: number;

  constructor(status: number) {
    super("Support report server rejected the request");
    this.status = status;
  }
}

export function redactErrorReport(draft: ErrorReportDraft): SupportReportRequest {
  if (!draft.consented) {
    throw new Error("Report consent is required");
  }
  return parseSupportReportRequest({
    traceId: draft.traceId,
    machineCode: draft.machineCode,
    route: draft.route,
    shell: draft.shell,
  });
}

function pendingReports(storage: Storage): SupportReportRequest[] {
  const stored = storage.getItem(PENDING_REPORTS_KEY);
  if (stored === null) {
    return [];
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(stored);
  } catch {
    storage.removeItem(PENDING_REPORTS_KEY);
    return [];
  }
  if (!Array.isArray(parsed)) {
    storage.removeItem(PENDING_REPORTS_KEY);
    return [];
  }
  const valid: SupportReportRequest[] = [];
  for (const entry of parsed) {
    try {
      valid.push(parseSupportReportRequest(entry));
    } catch {
      continue;
    }
  }
  if (valid.length !== parsed.length) {
    if (valid.length === 0) {
      storage.removeItem(PENDING_REPORTS_KEY);
    } else {
      storage.setItem(PENDING_REPORTS_KEY, JSON.stringify(valid));
    }
  }
  return valid;
}

function sameReport(left: SupportReportRequest, right: SupportReportRequest): boolean {
  return left.traceId === right.traceId && left.machineCode === right.machineCode;
}

async function postReport(report: SupportReportRequest): Promise<SavedReportReceipt> {
  const csrf = csrfTokenFromCookie(document.cookie);
  if (csrf === undefined) throw new ReportServerError(403);
  const response = await fetch("/api/support/reports", {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json", "x-layerx-csrf": csrf },
    body: JSON.stringify(report),
  });
  if (!response.ok) {
    throw new ReportServerError(response.status);
  }
  try {
    return parseSavedReportReceipt(await response.json());
  } catch {
    throw new ReportServerError(502);
  }
}

export class BrowserSupportReportOutbox implements ErrorReporter {
  #activeFlush: Promise<void> | undefined;

  async submit(draft: ErrorReportDraft): Promise<ErrorReportReceipt> {
    if (typeof window === "undefined") {
      throw new Error("Support reporting is available in the browser");
    }
    const report = redactErrorReport(draft);
    try {
      const receipt = await postReport(report);
      try {
        this.#removePending(report);
      } catch {
        return receipt;
      }
      return receipt;
    } catch (error) {
      if (error instanceof ReportServerError) {
        throw error;
      }
      this.#queuePending(report);
      return Object.freeze({ traceId: report.traceId, status: "pending" as const });
    }
  }

  flushPending(): Promise<void> {
    if (typeof window === "undefined" || !navigator.onLine) {
      return Promise.resolve();
    }
    if (this.#activeFlush !== undefined) {
      return this.#activeFlush;
    }
    const flush: Promise<void> = this.#drainPending().finally(() => {
      if (this.#activeFlush === flush) {
        this.#activeFlush = undefined;
      }
    });
    this.#activeFlush = flush;
    return flush;
  }

  #queuePending(report: SupportReportRequest): void {
    const reports = pendingReports(window.localStorage);
    if (reports.some((candidate) => sameReport(candidate, report))) {
      return;
    }
    window.localStorage.setItem(
      PENDING_REPORTS_KEY,
      JSON.stringify([...reports, report].slice(-MAX_PENDING_REPORTS)),
    );
  }

  #removePending(report: SupportReportRequest): void {
    const reports = pendingReports(window.localStorage).filter((candidate) => !sameReport(candidate, report));
    if (reports.length === 0) {
      window.localStorage.removeItem(PENDING_REPORTS_KEY);
    } else {
      window.localStorage.setItem(PENDING_REPORTS_KEY, JSON.stringify(reports));
    }
  }

  async #drainPending(): Promise<void> {
    for (const report of pendingReports(window.localStorage)) {
      await postReport(report);
      this.#removePending(report);
    }
  }
}

export const browserSupportReportOutbox = new BrowserSupportReportOutbox();
