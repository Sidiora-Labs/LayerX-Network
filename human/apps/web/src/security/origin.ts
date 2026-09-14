export function requestHasWebOrigin(request: Request, configuredOrigin: string | undefined): boolean {
  if (configuredOrigin === undefined || request.headers.get("sec-fetch-site") !== "same-origin") {
    return false;
  }
  const origin = request.headers.get("origin");
  if (origin === null) return false;
  try {
    const expected = new URL(configuredOrigin);
    const supplied = new URL(origin);
    const loopback = expected.hostname === "127.0.0.1" || expected.hostname === "[::1]";
    return (
      (expected.protocol === "https:" || (expected.protocol === "http:" && loopback)) &&
      expected.origin === configuredOrigin &&
      supplied.origin === origin &&
      supplied.origin === expected.origin
    );
  } catch {
    return false;
  }
}
