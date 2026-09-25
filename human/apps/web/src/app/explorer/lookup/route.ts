import { NextResponse, type NextRequest } from "next/server";

import { resolveName } from "../../../explorer/client";
import {
  accountIdentifierPath,
  parseAccountIdentifier,
  validExplorerIdentifier,
  validExplorerName,
} from "../../../explorer/model";

const DESTINATIONS = Object.freeze({
  receipt: "receipts",
  account: "accounts",
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
      new URL(accountIdentifierPath(resolved.did.toLowerCase()), request.url),
      303,
    );
  }
  if (kind === "account") {
    const account = parseAccountIdentifier(identifier);
    if (account === undefined) {
      return invalid(request, "invalid");
    }
    return NextResponse.redirect(new URL(accountIdentifierPath(account.canonical), request.url), 303);
  }
  if (!validExplorerIdentifier(identifier)) {
    return invalid(request, "invalid");
  }
  const destination = DESTINATIONS[kind as keyof typeof DESTINATIONS];
  return NextResponse.redirect(
    new URL(`/explorer/${destination}/${encodeURIComponent(normalised)}`, request.url),
    303,
  );
}
