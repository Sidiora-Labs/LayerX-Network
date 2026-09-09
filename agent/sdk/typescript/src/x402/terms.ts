import { PlatformSdkError } from "../production.js";

export type PaymentCommitment = "executed" | "batched" | "finalised";

function failure(): never {
  throw new PlatformSdkError({ code: "verification-failure", retry: "never" });
}

function layerxTerms(extra: unknown): Record<string, unknown> {
  if (extra === undefined || extra === null || typeof extra !== "object" || Array.isArray(extra)) return failure();
  const layerx = (extra as Record<string, unknown>)["layerx"];
  if (layerx === null || typeof layerx !== "object" || Array.isArray(layerx)) return failure();
  return layerx as Record<string, unknown>;
}

function hex64(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value) || /^0+$/u.test(value)) return failure();
  return value;
}

export function paymentCommitment(extra: unknown): PaymentCommitment {
  if (extra === undefined || extra === null || typeof extra !== "object" || Array.isArray(extra)) return "executed";
  const layerx = (extra as Record<string, unknown>)["layerx"];
  if (layerx === undefined) return "executed";
  if (layerx === null || typeof layerx !== "object" || Array.isArray(layerx)) return failure();
  const commitment = (layerx as Record<string, unknown>)["commitment"];
  if (commitment !== "executed" && commitment !== "batched" && commitment !== "finalised") return failure();
  return commitment;
}

export function paymentPayer(extra: unknown): string {
  return hex64(layerxTerms(extra)["payer"]);
}

export function grantPaymentTerms(extra: unknown): {
  readonly commitment: PaymentCommitment;
  readonly purposeHash: string;
  readonly payer: string;
} {
  const terms = layerxTerms(extra);
  if (!Object.hasOwn(terms, "commitment")) return failure();
  return {
    commitment: paymentCommitment(extra),
    purposeHash: hex64(terms["purposeHash"]),
    payer: hex64(terms["payer"]),
  };
}
