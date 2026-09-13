import { copyEntry } from "../../../copy/runtime.ts";
import type { EvidenceRef, Journey, JourneyStage, JourneyState } from "../../api/index.ts";
import type { StatusKey } from "../../kit/model";

export const ONBOARDING_DECISION_LIMIT = 3;
export const CREATE_ACCOUNT_DECISIONS = 2;
export const SIGN_IN_DECISIONS = 1;

export const PROTOCOL_IDENTITY_STAGE_KEY = "onboarding.stage.setting-up-your-protocol-identity";

export function journeyStatusKey(state: JourneyState): StatusKey {
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

export interface StagePresentation {
  readonly stageId: string;
  readonly copyKey: string;
  readonly title: string;
  readonly state: JourneyState;
  readonly status: StatusKey;
  readonly done: boolean;
  readonly queued: boolean;
}

export function stagePresentation(stage: JourneyStage): StagePresentation {
  const state = stage.state;
  return Object.freeze({
    stageId: stage.stage_id,
    copyKey: stage.copy_key,
    title: copyEntry(stage.copy_key).message,
    state,
    status: journeyStatusKey(state),
    done: state === "done" || state === "done-finalised",
    queued: state === "getting-ready",
  });
}

function verifiedReceipt(evidence: EvidenceRef): boolean {
  return evidence.class === "layerx-receipt"
    && (evidence.verification === "receipt-verified"
      || evidence.verification === "checkpoint-finalised"
      || evidence.verification === "paxeer-finalised");
}

export function accountActive(journey: Journey): boolean {
  const protocolStage = journey.stages.find((stage) => stage.copy_key === PROTOCOL_IDENTITY_STAGE_KEY);
  return protocolStage !== undefined && protocolStage.evidence.some(verifiedReceipt);
}

export function completedStageIds(journey: Journey): ReadonlySet<string> {
  return new Set(
    journey.stages
      .filter((stage) => stage.state === "done" || stage.state === "done-finalised")
      .map((stage) => stage.stage_id),
  );
}
