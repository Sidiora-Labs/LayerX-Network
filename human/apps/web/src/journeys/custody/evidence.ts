import type {
  EvidenceClass,
  Journey,
  JourneyStage,
  JourneyState,
  VerificationLevel,
} from "../../api/index.ts";
import type { StatusKey } from "../../kit/model.ts";

const VERIFICATION_ORDER = [
  "unverified",
  "receipt-verified",
  "checkpoint-finalised",
  "paxeer-finalised",
] as const satisfies readonly VerificationLevel[];

export function verificationAtLeast(level: VerificationLevel, required: VerificationLevel): boolean {
  return VERIFICATION_ORDER.indexOf(level) >= VERIFICATION_ORDER.indexOf(required);
}

interface StageEvidenceRule {
  readonly class?: EvidenceClass;
  readonly verification: VerificationLevel;
}

const STAGE_EVIDENCE_RULES: Readonly<Record<string, StageEvidenceRule>> = Object.freeze({
  "deposit.stage.waiting-for-wallet": Object.freeze({ class: "wallet-ack", verification: "unverified" }),
  "deposit.stage.linking-wallet": Object.freeze({ class: "layerx-receipt", verification: "receipt-verified" }),
  "deposit.stage.confirming-on-paxeer": Object.freeze({ class: "paxeer-finality", verification: "paxeer-finalised" }),
  "deposit.stage.crediting": Object.freeze({ class: "layerx-receipt", verification: "receipt-verified" }),
  "withdraw.stage.processing": Object.freeze({ class: "layerx-receipt", verification: "receipt-verified" }),
  "withdraw.stage.waiting-for-settlement": Object.freeze({ class: "checkpoint-proof", verification: "checkpoint-finalised" }),
  "withdraw.stage.ready-to-claim": Object.freeze({ class: "wallet-ack", verification: "unverified" }),
  "withdraw.stage.paying-out": Object.freeze({ class: "paxeer-finality", verification: "paxeer-finalised" }),
  "withdraw.stage.challenge-hold": Object.freeze({ class: "approval-hold", verification: "unverified" }),
  "exit.stage.getting-ready": Object.freeze({ verification: "unverified" }),
  "exit.stage.waiting-for-wallet": Object.freeze({ class: "wallet-ack", verification: "unverified" }),
  "exit.stage.confirming-on-paxeer": Object.freeze({ class: "paxeer-finality", verification: "paxeer-finalised" }),
});

export function stageEvidenceRule(copyKey: string): StageEvidenceRule | undefined {
  return STAGE_EVIDENCE_RULES[copyKey];
}

export function stageEvidenceBacked(stage: JourneyStage): boolean {
  const rule = stageEvidenceRule(stage.copy_key);
  if (rule === undefined) return false;
  return stage.evidence.some(
    (reference) =>
      (rule.class === undefined || reference.class === rule.class) &&
      verificationAtLeast(reference.verification, rule.verification),
  );
}

export function presentedStageState(stage: JourneyStage): JourneyState {
  if ((stage.state === "done" || stage.state === "done-finalised") && !stageEvidenceBacked(stage)) {
    return "still-checking";
  }
  return stage.state;
}

export function statusKeyForState(state: JourneyState): StatusKey {
  switch (state) {
    case "getting-ready":
      return "getting_ready";
    case "sending":
      return "sending";
    case "processing":
      return "processing";
    case "done":
      return "done";
    case "done-finalised":
      return "done_finalised";
    case "still-checking":
      return "still_checking";
    case "refused":
      return "refused";
    case "waiting-for-you":
      return "waiting_for_you";
  }
}

export function stageOutcomeVerified(journey: Journey, finalStageCopyKey: string): boolean {
  const stage = journey.stages.find((candidate) => candidate.copy_key === finalStageCopyKey);
  return (
    stage !== undefined &&
    (stage.state === "done" || stage.state === "done-finalised") &&
    stageEvidenceBacked(stage)
  );
}

export function presentedJourneyState(journey: Journey, finalStageCopyKey: string): JourneyState {
  if (
    (journey.state === "done" || journey.state === "done-finalised") &&
    !stageOutcomeVerified(journey, finalStageCopyKey)
  ) {
    return "still-checking";
  }
  return journey.state;
}

export function depositComplete(journey: Journey): boolean {
  return (
    (journey.state === "done" || journey.state === "done-finalised") &&
    stageOutcomeVerified(journey, "deposit.stage.crediting")
  );
}

export function withdrawPaidOut(journey: Journey): boolean {
  return (
    (journey.state === "done" || journey.state === "done-finalised") &&
    stageOutcomeVerified(journey, "withdraw.stage.paying-out")
  );
}

export function exitComplete(journey: Journey): boolean {
  if (journey.state !== "done" && journey.state !== "done-finalised") {
    return false;
  }
  if (stageOutcomeVerified(journey, "exit.stage.confirming-on-paxeer")) {
    return true;
  }
  return journey.evidence.some(
    (reference) =>
      reference.class === "paxeer-finality" &&
      verificationAtLeast(reference.verification, "paxeer-finalised"),
  );
}
