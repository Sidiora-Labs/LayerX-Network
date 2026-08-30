import type {
  EvidenceRef,
  HumanApiClient,
  Journey,
  SecurityActionKind,
  StepUpEvidence,
  TimedSecret,
} from "../../api/index.ts";
import {
  browserPasskeyAuthenticator,
  performStepUp,
  type PasskeyAuthenticator,
} from "../../journeys/approvals/ceremony.ts";

const RECOVERY_STAGE_COPY_KEY = "onboarding.stage.putting-recovery-in-place";

export interface RecoveryPresentation {
  readonly ready: boolean;
  readonly receipt?: EvidenceRef;
}

export function recoveryPresentation(journey: Journey): RecoveryPresentation {
  const stage = journey.stages.find((candidate) => candidate.copy_key === RECOVERY_STAGE_COPY_KEY);
  const receipt = stage?.evidence.find(
    (candidate) => candidate.class === "layerx-receipt"
      && candidate.verification !== "unverified",
  );
  return {
    ready: receipt !== undefined
      && (stage?.state === "done" || stage?.state === "done-finalised"),
    ...(receipt === undefined ? {} : { receipt }),
  };
}

export function formatLastActive(timestamp: string): string {
  const instant = new Date(timestamp);
  if (!Number.isFinite(instant.getTime())) {
    throw new TypeError("session activity timestamp is invalid");
  }
  return new Intl.DateTimeFormat(undefined, {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  }).format(instant);
}

export function secretExpiry(secret: TimedSecret): number {
  const expiry = Date.parse(secret.remask_at);
  if (!Number.isFinite(expiry)) {
    throw new TypeError("secret remask timestamp is invalid");
  }
  return expiry;
}

export async function securityStepUp(
  client: HumanApiClient,
  action: SecurityActionKind,
  targetId?: string,
  authenticator: PasskeyAuthenticator = browserPasskeyAuthenticator(),
): Promise<StepUpEvidence> {
  const binding = await client.securityAction({
    action,
    ...(targetId === undefined ? {} : { target_id: targetId }),
  });
  return performStepUp(client, binding.confirms, authenticator);
}

export const security = Object.freeze({
  recoveryPresentation,
  securityStepUp,
  formatLastActive,
  secretExpiry,
});

export function human_web_security() {
  return security;
}
