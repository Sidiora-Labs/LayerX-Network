import {
  SupportReportConfigurationError,
  SupportReportConflictError,
  supportReportRepositoryFromEnvironment,
  supportRetrievalAuthorized,
} from "../../../../../server/support-reports";
import { supportReportTrace } from "../../../../../states/report-schema";

export const runtime = "nodejs";

function problem(status: number, code: string, authenticate = false): Response {
  return Response.json(
    { code },
    {
      status,
      headers: {
        "cache-control": "no-store",
        ...(authenticate ? { "www-authenticate": "Bearer" } : {}),
      },
    },
  );
}

export async function GET(
  request: Request,
  context: Readonly<{ params: Promise<Readonly<{ traceId: string }>> }>,
): Promise<Response> {
  try {
    if (!supportRetrievalAuthorized(request)) {
      return problem(401, "REPORT_RETRIEVAL_UNAUTHORIZED", true);
    }
    const traceId = supportReportTrace((await context.params).traceId);
    const report = await supportReportRepositoryFromEnvironment().findByTrace(traceId);
    if (report === undefined) {
      return problem(404, "REPORT_NOT_FOUND");
    }
    return Response.json(report, { headers: { "cache-control": "no-store" } });
  } catch (error) {
    if (error instanceof SupportReportConfigurationError) {
      return problem(503, "REPORT_RETRIEVAL_UNAVAILABLE");
    }
    if (error instanceof SupportReportConflictError) {
      return problem(409, "REPORT_TRACE_CONFLICT");
    }
    if (error instanceof Error && /^Invalid trace/u.test(error.message)) {
      return problem(400, "REPORT_TRACE_INVALID");
    }
    return problem(500, "REPORT_RETRIEVAL_FAILED");
  }
}
