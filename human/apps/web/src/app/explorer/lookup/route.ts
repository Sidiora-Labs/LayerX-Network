import { NextResponse, type NextRequest } from "next/server";

import { resolveName } from "../../../explorer/client";
import {
  validExplorerCoordinate,
  validExplorerIdentifier,
  validExplorerName,
} from "../../../explorer/model";

const DESTINATIONS = Object.freeze({
  receipt: "receipts",
  account: "accounts",
  checkpoint: "checkpoints",
  batch: "batches",
  program: "programs",
});

function invalid(request: NextRequest, reason: string) {
  return NextResponse.redirect(new URL(`/explorer?lookup=${reason}`, request.url), 303);
}

export async function GET(request: NextRequest) {
  const kind = request.nextUrl.searchParams.get("kind");
  const identifier = request.nextUrl.searchParams.get("identifier")?.trim() ?? "";
  if (kind === null || !Object.hasOwn(DESTINATIONS, kind)) {
    return invalid(request, "invalid");
  }
  const normalised = identifier.normalize("NFC").toLowerCase();
  if (kind === "account" && validExplorerName(normalised)) {
    let resolved;
    try {
      resolved = await resolveName(normalised);
    } catch {
      return invalid(request, "unavailable");
    }
    if (resolved === undefined) {
      return invalid(request, "unknown-name");
    }
    return NextResponse.redirect(
      new URL(`/explorer/accounts/${encodeURIComponent(resolved.did)}`, request.url),
      303,
    );
  }
  const validIdentifier = kind === "batch"
    ? validExplorerCoordinate(identifier)
    : validExplorerIdentifier(identifier);
  if (!validIdentifier) {
    return invalid(request, "invalid");
  }
  const destination = DESTINATIONS[kind as keyof typeof DESTINATIONS];
  return NextResponse.redirect(
    new URL(`/explorer/${destination}/${encodeURIComponent(normalised)}`, request.url),
    303,
  );
}
