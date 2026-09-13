import { NextResponse } from "next/server";

import {
  performanceSinkSnapshot,
  persistPerformanceObservation,
} from "../../../../api/performance-sink";
import { RUM_SERIES_CAPACITY } from "../../../../perf/budgets";
import { parseRedactedWebVital } from "../../../../perf/rum-store";
import { requestHasWebOrigin } from "../../../../security/origin";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

const MAXIMUM_BODY_BYTES = 512;
const NO_STORE_HEADERS = { "cache-control": "no-store" } as const;

type BoundedBody =
  | Readonly<{ accepted: true; body: string }>
  | Readonly<{ accepted: false; status: 400 | 413 }>;

function requestIsJson(request: Request): boolean {
  const contentType = request.headers.get("content-type");
  return contentType?.split(";", 1)[0]?.trim().toLowerCase() === "application/json";
}

async function cancelBody(request: Request): Promise<void> {
  await request.body?.cancel().catch(() => undefined);
}

async function boundedBody(request: Request): Promise<BoundedBody> {
  const contentLength = request.headers.get("content-length");
  if (contentLength !== null) {
    if (!/^\d+$/u.test(contentLength)) {
      await cancelBody(request);
      return { accepted: false, status: 400 };
    }
    const declared = Number(contentLength);
    if (!Number.isSafeInteger(declared) || declared > MAXIMUM_BODY_BYTES) {
      await cancelBody(request);
      return { accepted: false, status: 413 };
    }
  }
  if (request.body === null) {
    return { accepted: true, body: "" };
  }
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) {
        break;
      }
      received += chunk.value.byteLength;
      if (received > MAXIMUM_BODY_BYTES) {
        await reader.cancel().catch(() => undefined);
        return { accepted: false, status: 413 };
      }
      chunks.push(chunk.value);
    }
  } catch {
    await reader.cancel().catch(() => undefined);
    return { accepted: false, status: 400 };
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  try {
    return { accepted: true, body: new TextDecoder("utf-8", { fatal: true }).decode(bytes) };
  } catch {
    return { accepted: false, status: 400 };
  }
}

export async function POST(request: Request) {
  if (!requestHasWebOrigin(request, process.env.LAYERX_HUMAN_WEB_ORIGIN)) {
    await cancelBody(request);
    return NextResponse.json({ accepted: false }, { status: 403, headers: NO_STORE_HEADERS });
  }
  if (!requestIsJson(request)) {
    await cancelBody(request);
    return NextResponse.json({ accepted: false }, { status: 415, headers: NO_STORE_HEADERS });
  }
  const bounded = await boundedBody(request);
  if (!bounded.accepted) {
    return NextResponse.json(
      { accepted: false },
      { status: bounded.status, headers: NO_STORE_HEADERS },
    );
  }
  let decoded: unknown;
  try {
    decoded = JSON.parse(bounded.body) as unknown;
  } catch {
    return NextResponse.json({ accepted: false }, { status: 400, headers: NO_STORE_HEADERS });
  }
  const observation = parseRedactedWebVital(decoded);
  if (observation === undefined) {
    return NextResponse.json({ accepted: false }, { status: 400, headers: NO_STORE_HEADERS });
  }
  const result = await persistPerformanceObservation(observation);
  return NextResponse.json(
    { accepted: result.accepted, durable: result.durable, sink: result.health },
    { status: result.accepted ? 202 : 503, headers: NO_STORE_HEADERS },
  );
}

export async function GET() {
  const snapshot = await performanceSinkSnapshot();
  return NextResponse.json(
    {
      version: 1,
      capacityPerSeries: RUM_SERIES_CAPACITY,
      sink: snapshot.health,
      aggregates: snapshot.aggregates,
    },
    { headers: NO_STORE_HEADERS },
  );
}
