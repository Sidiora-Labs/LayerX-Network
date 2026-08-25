import assert from "node:assert/strict";
import test from "node:test";

import type { JourneyStage } from "../src/api/index.ts";
import {
  presentedStageState,
  stageEvidenceBacked,
  stageEvidenceRule,
} from "../src/journeys/custody/evidence.ts";

test("unknown custody stages fail closed even with receipt evidence", () => {
  const stage: JourneyStage = {
    stage_id: "future-stage",
    copy_key: "withdraw.stage.future-payout",
    state: "done-finalised",
    evidence: [{
      evidence_id: "receipt-1",
      class: "layerx-receipt",
      verification: "paxeer-finalised",
    }],
  };

  assert.equal(stageEvidenceRule(stage.copy_key), undefined);
  assert.equal(stageEvidenceBacked(stage), false);
  assert.equal(presentedStageState(stage), "still-checking");
});
