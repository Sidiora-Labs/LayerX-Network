import { NextResponse, type NextRequest } from "next/server";

import {
  validExplorerCoordinate,
  validExplorerIdentifier,
} from "../../../explorer/model";

const DESTINATIONS = Object.freeze({
  receipt: "receipts",
  account: "accounts",
  checkpoint: "checkpoints",
  batch: "batches",
  program: "programs",
});

export function GET(request: NextRequest) {
  const kind = request.nextUrl.searchParams.get("kind");
  const identifier = request.nextUrl.searchParams.get("identifier")?.trim() ?? "";
  const validKind = kind !== null && Object.hasOwn(DESTINATIONS, kind);
  const validIdentifier = kind === "batch"
    ? validExplorerCoordinate(identifier)
    : validExplorerIdentifier(identifier);
  if (!validKind || !validIdentifier) {
    return NextResponse.redirect(new URL("/explorer?lookup=invalid", request.url), 303);
  }
  const destination = DESTINATIONS[kind as keyof typeof DESTINATIONS];
  return NextResponse.redirect(
    new URL(`/explorer/${destination}/${encodeURIComponent(identifier.toLowerCase())}`, request.url),
    303,
  );
}
