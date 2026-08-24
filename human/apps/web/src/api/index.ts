import { createHumanApiClient, type HumanApiClient, type HumanApiClientOptions } from "./generated/index.ts";

export * from "./generated/index.ts";

export function humanApi(options: HumanApiClientOptions = {}): HumanApiClient {
  return createHumanApiClient({
    credentials: "include",
    csrfToken: browserCsrfToken,
    trace: browserTrace,
    ...options,
  });
}

const CSRF_COOKIE = "__Host-layerx_csrf";

function browserCsrfToken(): string | undefined {
  if (typeof document === "undefined") {
    return undefined;
  }
  for (const entry of document.cookie.split(";")) {
    const [name, ...value] = entry.trim().split("=");
    if (name === CSRF_COOKIE) {
      const encoded = value.join("=");
      return encoded.length === 0 ? undefined : encoded;
    }
  }
  return undefined;
}

function browserTrace(): string | undefined {
  if (typeof globalThis.crypto?.getRandomValues !== "function") {
    return undefined;
  }
  const entropy = globalThis.crypto.getRandomValues(new Uint8Array(16));
  return `trc_${Array.from(entropy, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}
