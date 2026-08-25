export function privacyPrincipalScope(verifiedScope: string): string {
  if (!/^[A-Za-z0-9_-]{16,128}$/u.test(verifiedScope)) {
    throw new Error("Verified principal scope is invalid");
  }
  return verifiedScope;
}
