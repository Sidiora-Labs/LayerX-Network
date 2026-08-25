import { type NextRequest, NextResponse } from "next/server.js";

import { appContentSecurityPolicy } from "./security/csp.ts";

function nonce(): string {
  const bytes = new Uint8Array(18);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes));
}

export function proxy(request: NextRequest) {
  const requestNonce = nonce();
  const policy = appContentSecurityPolicy(requestNonce, process.env.NODE_ENV === "development");
  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("content-security-policy", policy);
  requestHeaders.set("x-nonce", requestNonce);

  const response = NextResponse.next({ request: { headers: requestHeaders } });
  response.headers.set("content-security-policy", policy);
  response.headers.set("accept-ch", "Viewport-Width");
  response.headers.set("critical-ch", "Viewport-Width");
  response.headers.set("referrer-policy", "same-origin");
  response.headers.set("x-content-type-options", "nosniff");
  response.headers.set("permissions-policy", "camera=(), microphone=(), geolocation=()");
  return response;
}

export const config = {
  matcher: ["/app/:path*"],
};
