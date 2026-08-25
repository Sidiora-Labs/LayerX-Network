import {
  SupportReportConfigurationError,
  SupportReportConflictError,
  supportReportRepositoryFromEnvironment,
} from "../../../../server/support-reports";
import { parseSupportReportRequest } from "../../../../states/report-schema";
import { validRequestCsrf, verifiedWebSession } from "../../../../auth/server-session";

export const runtime = "nodejs";

const MAX_REQUEST_BYTES = 4_096;

class ReportTooLargeError extends Error {}

function problem(status: number, code: string): Response {
  return Response.json({ code }, { status, headers: { "cache-control": "no-store" } });
}

async function boundedBody(request: Request): Promise<string> {
  if (request.body === null) {
    return "";
  }
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const result = await reader.read();
    if (result.done) {
      break;
    }
    total += result.value.byteLength;
    if (total > MAX_REQUEST_BYTES) {
      await reader.cancel();
      throw new ReportTooLargeError("Support report exceeds the request limit");
    }
    chunks.push(result.value);
  }
  const body = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(body);
}

export async function POST(request: Request): Promise<Response> {
  const session = await verifiedWebSession(request.headers.get("cookie") ?? "");
  if (session === undefined) {
    return problem(401, "REPORT_SESSION_REQUIRED");
  }
  if (!validRequestCsrf(request)) {
    return problem(403, "REPORT_CSRF_REJECTED");
  }
  if (!request.headers.get("content-type")?.toLowerCase().startsWith("application/json")) {
    return problem(415, "REPORT_CONTENT_TYPE_REQUIRED");
  }
  const declaredLength = Number(request.headers.get("content-length") ?? "0");
  if (Number.isFinite(declaredLength) && declaredLength > MAX_REQUEST_BYTES) {
    return problem(413, "REPORT_TOO_LARGE");
  }

  try {
    const body = await boundedBody(request);
    const report = parseSupportReportRequest(JSON.parse(body));
    const saved = await supportReportRepositoryFromEnvironment().save(session.principalScope, report);
    return Response.json(
      { traceId: saved.record.traceId, status: "saved" },
      { status: saved.created ? 201 : 200, headers: { "cache-control": "no-store" } },
    );
  } catch (error) {
    if (error instanceof ReportTooLargeError) {
      return problem(413, "REPORT_TOO_LARGE");
    }
    if (error instanceof SupportReportConfigurationError) {
      return problem(503, "REPORT_STORAGE_UNAVAILABLE");
    }
    if (error instanceof SupportReportConflictError) {
      return problem(409, "REPORT_TRACE_CONFLICT");
    }
    if (error instanceof SyntaxError || error instanceof TypeError || error instanceof Error && /^Invalid|must be|Support report fields/u.test(error.message)) {
      return problem(400, "REPORT_INVALID");
    }
    return problem(500, "REPORT_SAVE_FAILED");
  }
}
