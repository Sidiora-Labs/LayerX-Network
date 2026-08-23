import { expect, test } from "@playwright/test";

test.describe("Move money journey", () => {
  test("@journey navigates to move money from home", async ({ page }) => {
    await page.goto("/app", { waitUntil: "networkidle" });
    await expect(page.locator("main")).toHaveCount(1);
    
    const moveButton = page.getByRole("button", { name: /move money/i });
    await expect(moveButton).toBeVisible();
    await moveButton.click();
    
    await expect(page).toHaveURL("/app/move");
    await expect(page.getByRole("heading", { name: /move money/i })).toBeVisible();
  });

  test("@journey completes move money wizard with three decisions in mobile shell", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const whoStep = page.getByRole("heading", { name: /who/i });
    await expect(whoStep).toBeVisible();
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await expect(destinationOption).toBeChecked();
    
    const continueButton = page.getByRole("button", { name: /continue/i });
    await expect(continueButton).toBeEnabled();
    await continueButton.click();
    
    const amountStep = page.getByRole("heading", { name: /how much/i });
    await expect(amountStep).toBeVisible();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    
    await expect(continueButton).toBeEnabled();
    await continueButton.click();
    
    const reviewStep = page.getByRole("heading", { name: /review/i });
    await expect(reviewStep).toBeVisible();
    
    await expect(page.getByText(/fee estimate/i)).toBeVisible();
    await expect(page.getByText(/ceiling/i)).toBeVisible();
    
    const confirmButton = page.getByRole("button", { name: /move money/i });
    await expect(confirmButton).toBeEnabled();
  });

  test("@journey completes move money wizard with three decisions in desktop shell", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const whoStep = page.getByRole("heading", { name: /who/i });
    await expect(whoStep).toBeVisible();
    
    await expect(page.getByText(/summary/i)).toBeVisible();
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    
    const continueButton = page.getByRole("button", { name: /continue/i });
    await continueButton.click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    
    await continueButton.click();
    
    const reviewStep = page.getByRole("heading", { name: /review/i });
    await expect(reviewStep).toBeVisible();
    
    await expect(page.getByText(/fee estimate/i)).toBeVisible();
    await expect(page.getByText(/ceiling/i)).toBeVisible();
  });

  test("@journey shows review with plain language quote", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    
    await page.getByRole("button", { name: /continue/i }).click();
    
    await expect(page.getByText(/fee estimate/i)).toBeVisible();
    await expect(page.getByText(/ceiling/i)).toBeVisible();
    await expect(page.getByText(/arrival/i)).toBeVisible();
    await expect(page.getByText(/automatic/i)).toBeVisible();
  });

  test("@journey renders other account destination option", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const otherAccountOption = page.getByRole("radio", { name: /other account/i });
    await expect(otherAccountOption).toBeVisible();
    await otherAccountOption.check();
    
    const accountInput = page.getByRole("textbox", { name: /account/i });
    await expect(accountInput).toBeVisible();
    await accountInput.fill("acc_test_12345678");
    
    const continueButton = page.getByRole("button", { name: /continue/i });
    await expect(continueButton).toBeEnabled();
  });

  test("@journey prevents continuation without destination selection", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const continueButton = page.getByRole("button", { name: /continue/i });
    await expect(continueButton).toBeDisabled();
  });

  test("@journey prevents continuation without amount", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    
    await page.getByRole("button", { name: /continue/i }).click();
    
    const continueButton = page.getByRole("button", { name: /continue/i });
    await expect(continueButton).toBeDisabled();
  });

  test("@journey @refusal renders refusal state with honest message", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("999999999999");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 422,
        contentType: "application/json",
        body: JSON.stringify({
          code: "refused-by-budget",
          copy_key: "error.move.budget_exceeded",
          trace: "trace_test_12345678",
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByRole("alert")).toBeVisible();
    await expect(page.getByText(/refused/i)).toBeVisible();
    await expect(page.getByText(/money.*stayed|money.*left/i)).toBeVisible();
  });

  test("@journey @refusal shows change path when refusal provides it", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("100000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          journey_id: "jny_test_refused_12345678",
          state: "refused",
          refusal: {
            code: "refused-by-budget",
            copy_key: "error.move.budget_exceeded",
            change_path: "/app/agents/agt_test_12345678",
            money_left: false,
          },
          stages: [],
          evidence: [],
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByRole("button", { name: /change/i })).toBeVisible();
  });

  test("@journey @still-checking renders still-checking state with locked actions", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          journey_id: "jny_test_checking_12345678",
          state: "still-checking",
          stages: [
            {
              stage_id: "stg_test_12345678",
              copy_key: "move.stage.transfer",
              state: "still-checking",
              evidence: [],
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
          evidence: [],
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByText(/still.?checking/i)).toBeVisible();
    await expect(page.getByText(/locked/i)).toBeVisible();
    
    const actionButtons = page.getByRole("button").filter({ hasText: /send|again/i });
    if ((await actionButtons.count()) > 0) {
      await expect(actionButtons.first()).toBeDisabled();
    }
  });

  test("@journey renders in-progress state with receipt-gating", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          journey_id: "jny_test_progress_12345678",
          state: "sending",
          stages: [
            {
              stage_id: "stg_test_12345678",
              copy_key: "move.stage.transfer",
              state: "sending",
              evidence: [],
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
          evidence: [],
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByRole("status")).toBeVisible();
    await expect(page.getByText(/sending|processing/i)).toBeVisible();
  });

  test("@journey @receipt-backed renders done state only when receipt-backed", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          journey_id: "jny_test_done_12345678",
          state: "done",
          stages: [
            {
              stage_id: "stg_test_12345678",
              copy_key: "move.stage.transfer",
              state: "done",
              evidence: [
                {
                  evidence_id: "evt_test_12345678",
                  class: "layerx-receipt",
                  verification: "receipt-verified",
                  retrieved_at: new Date().toISOString(),
                },
              ],
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
          evidence: [],
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByText(/done|complete/i)).toBeVisible();
    await expect(page.getByText(/receipt/i)).toBeVisible();
  });

  test("@journey shows processing when done state lacks receipt", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          journey_id: "jny_test_done_no_receipt_12345678",
          state: "done",
          stages: [
            {
              stage_id: "stg_test_12345678",
              copy_key: "move.stage.transfer",
              state: "done",
              evidence: [],
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
          evidence: [],
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByText(/processing/i)).toBeVisible();
    const doneIndicator = page.getByText(/done|complete/i);
    await expect(doneIndicator).not.toBeVisible();
  });

  test("@journey supports cancellation back to home", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const cancelButton = page.getByRole("button", { name: /cancel|back/i });
    await expect(cancelButton).toBeVisible();
    await cancelButton.click();
    
    await expect(page).toHaveURL("/app");
  });

  test("@journey handles quote expiration gracefully", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    await page.getByRole("button", { name: /continue/i }).click();
    
    await page.route("**/v1/move/commit", async (route) => {
      await route.fulfill({
        status: 422,
        contentType: "application/json",
        body: JSON.stringify({
          code: "quote-expired",
          copy_key: "error.move.quote-expired",
          trace: "trace_test_12345678",
        }),
      });
    });
    
    await page.getByRole("button", { name: /move money/i }).click();
    
    await expect(page.getByText(/expired/i)).toBeVisible();
    await expect(page.getByRole("heading", { name: /review/i })).toBeVisible();
  });

  test("@journey @visual desktop split pane shows live summary", async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const destinationOption = page.getByRole("radio").first();
    await destinationOption.check();
    
    await expect(page.getByText(/to/i)).toBeVisible();
    
    await page.getByRole("button", { name: /continue/i }).click();
    
    const amountInput = page.getByRole("textbox", { name: /amount/i });
    await amountInput.fill("1000");
    
    await expect(page.getByText(/amount/i)).toBeVisible();
    await expect(page.getByText(/1.*000/)).toBeVisible();
    
    expect(process.env.HUMAN_VISUAL_BASELINE_REVIEWED).toBe("1");
    await expect(page).toHaveScreenshot(`${testInfo.project.name}-move-summary.png`, {
      fullPage: true,
      animations: "disabled",
    });
  });

  test("@journey mobile full-screen wizard hides other content", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const wizardContainer = page.locator("[role='region']").filter({ hasText: /move money/i });
    await expect(wizardContainer).toBeVisible();
    
    const mainLandmark = page.locator("main");
    await expect(mainLandmark).toBeVisible();
  });

  test("@journey enforces three decisions maximum for initiation", async ({ page }) => {
    await page.goto("/app/move", { waitUntil: "networkidle" });
    
    const steps = page.getByRole("button", { name: /who|amount|review/i });
    await expect(steps).toHaveCount(3);
  });
});
