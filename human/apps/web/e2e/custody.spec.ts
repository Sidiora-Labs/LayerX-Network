import assert from "node:assert/strict";
import test from "node:test";

import { CUSTODY_CURRENCY } from "../src/journeys/custody/model.ts";
import {
  DEPOSIT_FINAL_STAGE,
  DepositController,
  depositPlan,
} from "../src/journeys/deposit/model.ts";
import {
  WITHDRAW_FINAL_STAGE,
  WithdrawController,
  withdrawPlan,
  challengeHoldPresentation,
  settlementPresentation,
} from "../src/journeys/withdraw/model.ts";
import {
  EXIT_CONFIRMATION_PHRASE,
  EXIT_FINAL_STAGE,
  ExitController,
  exitPlan,
  exitConfirmationReady,
} from "../src/journeys/exit/model.ts";
import { presentedJourneyState } from "../src/journeys/custody/evidence.ts";
import type { Journey, WalletBinding, ExitEligibility, WalletSignRequest } from "../src/api/index.ts";
import type { PaxeerWalletBridge } from "../src/journeys/custody/handoff.ts";
import type { CustodyTiming } from "../src/journeys/custody/time.ts";

const mockBridge: PaxeerWalletBridge = {
  async sign(_request: WalletSignRequest) {
    return { outcome: "approved", reference: "0xmock" };
  },
};

const mockApi = {
  async depositStart() {
    return mockDepositJourney();
  },
  async depositConfirm() {
    return mockDepositJourney();
  },
  async withdrawStart() {
    return mockWithdrawJourney();
  },
  async withdrawClaim() {
    return mockWithdrawJourney();
  },
  async exitStart() {
    return mockExitJourney();
  },
  async exitEligibility() {
    return mockExitEligibilityEligible();
  },
  async journeyGet() {
    return mockDepositJourney();
  },
  async bindingStatus() {
    return mockWalletBinding();
  },
} as const;

function mockWalletBinding(): WalletBinding {
  return {
    state: "bound",
    address: "0x1234567890123456789012345678901234567890",
    linked_at: Date.now() - 3600000,
  };
}

function mockCustodyTiming(): CustodyTiming {
  return {
    depositDelayedAfterSeconds: 120,
    settlement: {
      estimatedEpochCount: 2,
      epochLengthSeconds: 1200,
    },
    challengeWindowSeconds: 3600,
  };
}

function mockDepositJourney(): Journey {
  return {
    journey_id: "jrn_deposit_test",
    kind: "deposit",
    state: "done",
    created_at: Date.now() - 600000,
    updated_at: Date.now() - 300000,
    stages: [
      {
        stage_id: "stage_wallet",
        copy_key: "deposit.stage.waiting-for-wallet",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_confirming",
        copy_key: "deposit.stage.confirming-on-paxeer",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_crediting",
        copy_key: DEPOSIT_FINAL_STAGE,
        state: "done",
        evidence: [{ evidence_id: "evt_credit", class: "credit", verification: "sequencer_signed" }],
      },
    ],
    evidence: [],
  };
}

function mockWithdrawJourney(): Journey {
  return {
    journey_id: "jrn_withdraw_test",
    kind: "withdraw",
    state: "done",
    created_at: Date.now() - 7200000,
    updated_at: Date.now() - 300000,
    stages: [
      {
        stage_id: "stage_processing",
        copy_key: "withdraw.stage.processing",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_settlement",
        copy_key: "withdraw.stage.waiting-for-settlement",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_claim",
        copy_key: "withdraw.stage.ready-to-claim",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_payout",
        copy_key: WITHDRAW_FINAL_STAGE,
        state: "done",
        evidence: [{ evidence_id: "evt_payout", class: "payout", verification: "checkpoint_finalised" }],
      },
    ],
    evidence: [],
  };
}

function mockWithdrawJourneyWithHold(): Journey {
  return {
    journey_id: "jrn_withdraw_hold_test",
    kind: "withdraw",
    state: "processing",
    created_at: Date.now() - 7200000,
    updated_at: Date.now() - 60000,
    stages: [
      {
        stage_id: "stage_processing",
        copy_key: "withdraw.stage.processing",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_settlement",
        copy_key: "withdraw.stage.waiting-for-settlement",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_claim",
        copy_key: "withdraw.stage.ready-to-claim",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_hold",
        copy_key: "withdraw.stage.challenge-hold",
        state: "processing",
        evidence: [],
      },
      {
        stage_id: "stage_payout",
        copy_key: WITHDRAW_FINAL_STAGE,
        state: "getting-ready",
        evidence: [],
      },
    ],
    evidence: [],
  };
}

function mockExitJourney(): Journey {
  return {
    journey_id: "jrn_exit_test",
    kind: "exit",
    state: "done",
    created_at: Date.now() - 600000,
    updated_at: Date.now() - 300000,
    stages: [
      {
        stage_id: "stage_getting_ready",
        copy_key: "exit.stage.getting-ready",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_wallet",
        copy_key: "exit.stage.waiting-for-wallet",
        state: "done",
        evidence: [],
      },
      {
        stage_id: "stage_confirming",
        copy_key: EXIT_FINAL_STAGE,
        state: "done",
        evidence: [{ evidence_id: "evt_exit", class: "exit", verification: "checkpoint_finalised" }],
      },
    ],
    evidence: [],
  };
}

function mockExitEligibilityEligible(): ExitEligibility {
  return {
    eligible: true,
  };
}

function mockExitEligibilityUnavailable(): ExitEligibility {
  return {
    eligible: false,
    copy_key: "exit.unavailable.network-operating-normally",
    withdraw_instead_path: "/app/withdraw",
  };
}

function mockRandomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    bytes[i] = i;
  }
  return bytes;
}

test("deposit plan shows binding-folded start when no wallet is bound", () => {
  const plan = depositPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "form");
  assert.equal(plan.bindingFolded, true);
  assert.equal(plan.amount.value, "100");
  assert.equal(plan.amount.errorKey, undefined);
  assert.equal(plan.primaryAction.disabled, false);
});

test("deposit plan validates positive amount and shows inline error", () => {
  const planInvalid = depositPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "0",
    walletPhase: "idle",
  });
  assert.equal(planInvalid.amount.errorKey, "deposit.amount.invalid");
  assert.equal(planInvalid.primaryAction.disabled, true);

  const planValid = depositPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    walletPhase: "idle",
  });
  assert.equal(planValid.amount.errorKey, undefined);
  assert.equal(planValid.primaryAction.disabled, false);
});

test("deposit journey plan shows staged timeline with wallet hand-off states", () => {
  const journey = mockDepositJourney();
  const plan = depositPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    binding: mockWalletBinding(),
    journey,
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "journey");
  assert.equal(plan.timeline !== undefined, true);
  assert.equal(plan.timeline!.length, 3);
  assert.equal(plan.complete !== undefined, true);
  assert.equal(plan.complete!.titleKey, "deposit.complete");
});

test("deposit journey shows safe-to-close notice and pending honesty while active", () => {
  const journey = {
    ...mockDepositJourney(),
    state: "processing" as const,
    stages: mockDepositJourney().stages.map((stage, idx) =>
      idx === 2 ? { ...stage, state: "processing" as const } : stage,
    ),
  };
  const plan = depositPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    binding: mockWalletBinding(),
    journey,
    walletPhase: "approved",
  });
  assert.equal(plan.safeToCloseKey, "deposit.safe_to_close");
  assert.equal(plan.pendingHonestyKey, "deposit.pending.not_counted");
  assert.equal(plan.complete, undefined);
});

test("deposit controller manages wallet hand-off phases", async () => {
  const controller = new DepositController({
    api: mockApi as any,
    bridge: mockBridge,
    randomBytes: mockRandomBytes,
  });
  assert.equal(controller.walletPhase, "idle");
  assert.equal(controller.journey, undefined);
  assert.equal(controller.idempotencyKey, undefined);

  await controller.start(100n, CUSTODY_CURRENCY);
  assert.equal(controller.journey !== undefined, true);
  assert.equal(controller.journey!.kind, "deposit");
  assert.equal(controller.idempotencyKey !== undefined, true);
  assert.equal(controller.idempotencyKey!.length, 32);
});

test("withdraw plan shows irreversibility warning at review", () => {
  const plan = withdrawPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0x1234567890123456789012345678901234567890",
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "form");
  assert.equal(plan.review.irreversibleKey, "withdraw.irreversible");
  assert.equal(plan.review.commitKey, "withdraw.commit");
  assert.equal(plan.review.ready, true);
});

test("withdraw plan shows settlement expectation derived from configuration", () => {
  const timing = mockCustodyTiming();
  const settlement = settlementPresentation(timing);
  assert.equal(settlement.bodyKey, "withdraw.settlement.expectation");
  assert.equal(settlement.duration !== undefined, true);

  const timingUndeclared: CustodyTiming = { depositDelayedAfterSeconds: 120 };
  const settlementUndeclared = settlementPresentation(timingUndeclared);
  assert.equal(settlementUndeclared.bodyKey, "withdraw.settlement.undeclared");
  assert.equal(settlementUndeclared.duration, undefined);
});

test("withdraw plan validates destination address and shows inline error", () => {
  const planInvalid = withdrawPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "invalid-address",
    walletPhase: "idle",
  });
  assert.equal(planInvalid.destination.errorKey, "withdraw.destination.invalid");

  const planValid = withdrawPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0xAbCdEf1234567890123456789012345678901234",
    walletPhase: "idle",
  });
  assert.equal(planValid.destination.errorKey, undefined);
});

test("withdraw journey plan shows staged timeline with claim readiness", () => {
  const journey = mockWithdrawJourney();
  const plan = withdrawPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0x1234567890123456789012345678901234567890",
    journey,
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "journey");
  assert.equal(plan.timeline !== undefined, true);
  assert.equal(plan.timeline!.length, 4);
  assert.equal(plan.complete !== undefined, true);
  assert.equal(plan.complete!.titleKey, "status.paid_out");
  assert.equal(plan.complete!.bodyKey, "withdraw.paid_out.body");
});

test("withdraw journey shows challenge-window hold as an honest state with expectations", () => {
  const journey = mockWithdrawJourneyWithHold();
  const timing = mockCustodyTiming();
  const hold = challengeHoldPresentation(journey, timing);
  assert.equal(hold !== undefined, true);
  assert.equal(hold!.titleKey, "withdraw.hold.title");
  assert.equal(hold!.bodyKey, "withdraw.hold.body");
  assert.equal(hold!.expectation !== undefined, true);
  assert.equal(hold!.expectation!.bodyKey, "withdraw.hold.expectation");
  assert.equal(hold!.expectation!.duration !== undefined, true);
  assert.equal(hold!.cancelledKey, undefined);

  const plan = withdrawPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0x1234567890123456789012345678901234567890",
    journey,
    walletPhase: "idle",
  });
  assert.equal(plan.hold !== undefined, true);
  assert.equal(plan.hold!.titleKey, "withdraw.hold.title");
});

test("withdraw controller manages commit to claim flow", async () => {
  const controller = new WithdrawController({
    api: mockApi as any,
    bridge: mockBridge,
    randomBytes: mockRandomBytes,
  });
  assert.equal(controller.walletPhase, "idle");
  assert.equal(controller.journey, undefined);

  await controller.commit(50n, CUSTODY_CURRENCY, "0x1234567890123456789012345678901234567890");
  assert.equal(controller.journey !== undefined, true);
  assert.equal(controller.journey!.kind, "withdraw");
  assert.equal(controller.idempotencyKey !== undefined, true);
});

test("exit plan requires typed confirmation with exact phrase", () => {
  const planNotReady = exitPlan({
    shell: "desktop",
    typedConfirmation: "incorrect",
    degraded: false,
    eligibility: mockExitEligibilityEligible(),
    walletPhase: "idle",
  });
  assert.equal(planNotReady.phase, "confirm");
  assert.equal(planNotReady.confirmation !== undefined, true);
  assert.equal(planNotReady.confirmation!.expectedValue, EXIT_CONFIRMATION_PHRASE);
  assert.equal(planNotReady.confirmation!.ready, false);

  const planReady = exitPlan({
    shell: "desktop",
    typedConfirmation: EXIT_CONFIRMATION_PHRASE,
    degraded: false,
    eligibility: mockExitEligibilityEligible(),
    walletPhase: "idle",
  });
  assert.equal(planReady.confirmation!.ready, true);
  assert.equal(exitConfirmationReady(EXIT_CONFIRMATION_PHRASE), true);
  assert.equal(exitConfirmationReady("wrong"), false);
});

test("exit plan shows unavailable state when network operates normally", () => {
  const plan = exitPlan({
    shell: "mobile",
    typedConfirmation: "",
    degraded: false,
    eligibility: mockExitEligibilityUnavailable(),
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "unavailable");
  assert.equal(plan.unavailable !== undefined, true);
  assert.equal(plan.unavailable!.bodyKey, "exit.unavailable.network-operating-normally");
  assert.equal(plan.unavailable!.withdrawInsteadPath, "/app/withdraw");
  assert.equal(plan.unavailable!.withdrawInsteadKey, "exit.withdraw_instead");
});

test("exit plan shows degraded-mode operability when service is degraded", () => {
  const plan = exitPlan({
    shell: "desktop",
    typedConfirmation: EXIT_CONFIRMATION_PHRASE,
    degraded: true,
    eligibility: mockExitEligibilityEligible(),
    walletPhase: "idle",
  });
  assert.equal(plan.degradedKey, "exit.degraded");
});

test("exit journey plan shows staged timeline with completion gated on finality", () => {
  const journey = mockExitJourney();
  const plan = exitPlan({
    shell: "desktop",
    typedConfirmation: "",
    degraded: false,
    journey,
    walletPhase: "idle",
  });
  assert.equal(plan.phase, "journey");
  assert.equal(plan.timeline !== undefined, true);
  assert.equal(plan.timeline!.length, 3);
  assert.equal(plan.complete !== undefined, true);
  assert.equal(plan.complete!.titleKey, "exit.complete");
  assert.equal(plan.complete!.bodyKey, "exit.complete.body");
});

test("exit controller checks eligibility and manages typed start flow", async () => {
  const controller = new ExitController({
    api: mockApi as any,
    bridge: mockBridge,
    randomBytes: mockRandomBytes,
  });
  assert.equal(controller.eligibility, undefined);
  assert.equal(controller.journey, undefined);

  await controller.checkEligibility();
  assert.equal(controller.eligibility !== undefined, true);
  assert.equal(controller.eligibility!.eligible, true);

  await controller.start(EXIT_CONFIRMATION_PHRASE);
  assert.equal(controller.journey !== undefined, true);
  assert.equal(controller.journey!.kind, "exit");
});

test("custody journey state presentation enforces receipt-gated completion", () => {
  const journeyWithoutEvidence = mockDepositJourney();
  journeyWithoutEvidence.stages[2].evidence = [];
  const presentedWithoutEvidence = presentedJourneyState(journeyWithoutEvidence, DEPOSIT_FINAL_STAGE);
  assert.equal(presentedWithoutEvidence, "processing");

  const journeyWithEvidence = mockDepositJourney();
  const presentedWithEvidence = presentedJourneyState(journeyWithEvidence, DEPOSIT_FINAL_STAGE);
  assert.equal(presentedWithEvidence, "done");
});

test("custody journeys lock duplicate actions during still-checking states", () => {
  const stillCheckingJourney = {
    ...mockDepositJourney(),
    state: "still-checking" as const,
  };
  const plan = depositPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    binding: mockWalletBinding(),
    journey: stillCheckingJourney,
    walletPhase: "idle",
  });
  assert.equal(plan.duplicateLocked, true);
  assert.equal(plan.primaryAction.disabled, true);
  assert.equal(plan.primaryAction.disabledReasonKey, "state.still_checking.locked");
});

test("both shells produce the same plan structure with shell-specific layouts", () => {
  const desktopPlan = depositPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    binding: mockWalletBinding(),
    walletPhase: "idle",
  });
  const mobilePlan = depositPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "100",
    binding: mockWalletBinding(),
    walletPhase: "idle",
  });
  assert.equal(desktopPlan.phase, mobilePlan.phase);
  assert.equal(desktopPlan.bindingFolded, mobilePlan.bindingFolded);
  assert.equal(desktopPlan.titleKey, mobilePlan.titleKey);
  assert.equal(desktopPlan.summaryItems.length > mobilePlan.summaryItems.length, true);
});

test("withdraw wizard steps match both shells with different layouts", () => {
  const desktopPlan = withdrawPlan({
    shell: "desktop",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0x1234567890123456789012345678901234567890",
    walletPhase: "idle",
  });
  const mobilePlan = withdrawPlan({
    shell: "mobile",
    timing: mockCustodyTiming(),
    nowMs: Date.now(),
    amountInput: "50",
    destinationInput: "0x1234567890123456789012345678901234567890",
    walletPhase: "idle",
  });
  assert.equal(desktopPlan.review.titleKey, mobilePlan.review.titleKey);
  assert.equal(desktopPlan.review.irreversibleKey, mobilePlan.review.irreversibleKey);
  assert.equal(desktopPlan.settlement.bodyKey, mobilePlan.settlement.bodyKey);
});

test("exit typed confirmation works identically across both shells", () => {
  const desktopPlan = exitPlan({
    shell: "desktop",
    typedConfirmation: EXIT_CONFIRMATION_PHRASE,
    degraded: false,
    eligibility: mockExitEligibilityEligible(),
    walletPhase: "idle",
  });
  const mobilePlan = exitPlan({
    shell: "mobile",
    typedConfirmation: EXIT_CONFIRMATION_PHRASE,
    degraded: false,
    eligibility: mockExitEligibilityEligible(),
    walletPhase: "idle",
  });
  assert.equal(desktopPlan.confirmation!.ready, mobilePlan.confirmation!.ready);
  assert.equal(desktopPlan.confirmation!.expectedValue, mobilePlan.confirmation!.expectedValue);
  assert.equal(desktopPlan.confirmation!.consequenceKey, mobilePlan.confirmation!.consequenceKey);
});
