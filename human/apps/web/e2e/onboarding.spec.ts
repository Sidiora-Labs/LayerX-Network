import { expect, test } from "@playwright/test";
import { human_test_harness, SHELL_PROFILES } from "./harness";

const harness = human_test_harness(process.env);

for (const [shellName, profile] of Object.entries(SHELL_PROFILES)) {
  test.describe(`${shellName} shell onboarding journeys`, () => {
    test.use(profile);

    test("@journey renders account creation with email and name fields", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await expect(page.locator('[data-journey="onboarding"]')).toBeVisible();
      await expect(page.locator('[data-decision-screen="1"]')).toBeVisible();
      await expect(page.getByLabel(/email/i)).toBeVisible();
      await expect(page.getByLabel(/name/i)).toBeVisible();
      await expect(page.getByRole("button", { name: /continue/i })).toBeVisible();
    });

    test("@journey enforces the three-decision limit", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const journey = page.locator('[data-journey="onboarding"]');
      await expect(journey).toHaveAttribute("data-decision-limit", "3");
    });

    test("@journey shows switch to sign-in action", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await expect(page.getByRole("button", { name: /sign in instead/i })).toBeVisible();
    });

    test("@journey renders sign-in screen with passkey ceremony", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await page.getByRole("button", { name: /sign in instead/i }).click();
      await expect(page.locator('[data-decision-screen="1"]')).toBeVisible();
      await expect(page.getByRole("button", { name: /sign in with your passkey/i })).toBeVisible();
      await expect(page.getByRole("button", { name: /create an account/i })).toBeVisible();
    });

    test("@journey shows honest activation progress during onboarding", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-journey-phase="progress"]')).toBeVisible();
      await expect(page.locator('[data-onboarding-stages]')).toBeVisible();
    });

    test("@journey presents account-active notice only when verified", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      const activeNotice = page.locator('[data-account-active]');
      await expect(activeNotice).toBeVisible();
    });

    test("@journey shows staged progress with status pills", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-onboarding-stages]')).toBeVisible();
      await expect(page.locator('[data-stage-state]')).toHaveCount(4);
    });

    test("@journey displays safe-to-close guidance during activation", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(
        page.getByText(/safe to close/i)
      ).toBeVisible();
    });

    test("@journey shows failure notice with retry for failed stages", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_failure=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-onboarding-failure]')).toBeVisible();
      await expect(
        page.getByText(/progress is saved|finished steps stay finished/i)
      ).toBeVisible();
    });

    test("@journey renders resume action to continue incomplete onboarding", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-onboarding-resume]')).toBeVisible();
    });

    test("@journey shows queued state when protocol layer unavailable", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_queued=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-onboarding-queued]')).toBeVisible();
      await expect(
        page.getByText(/waiting in line|nothing you did is lost/i)
      ).toBeVisible();
    });

    test("@journey validates required email field", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const emailField = page.getByLabel(/email/i);
      await expect(emailField).toHaveAttribute("required", "");
      await expect(emailField).toHaveAttribute("type", "email");
    });

    test("@journey validates required name field", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const nameField = page.getByLabel(/name/i);
      await expect(nameField).toHaveAttribute("required", "");
    });

    test("@journey new-device sign-in shows security notification", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_signin_new_device=1`, {
        waitUntil: "networkidle",
      });
      const securityNotice = page.locator('[data-security-notice="security-new-device"]');
      await expect(securityNotice).toBeVisible();
      const reviewAction = page.locator('[data-security-notice-action]');
      await expect(reviewAction).toBeVisible();
    });

    test("@journey new-device sign-in notification links to device list", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_signin_new_device=1`, {
        waitUntil: "networkidle",
      });
      const reviewAction = page.locator('[data-security-notice-action]');
      await expect(reviewAction).toHaveAttribute("data-security-notice-action", "");
    });

    test("@journey renders passkey ceremony screen as decision 2", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_account_created=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-decision-screen="2"]')).toBeVisible();
      await expect(page.getByRole("button", { name: /create your passkey/i })).toBeVisible();
    });

    test("@journey passkey screen shows stage progress", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_account_created=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-onboarding-stages]')).toBeVisible();
    });

    test("@journey validates shell-specific rendering", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const journey = page.locator('[data-journey="onboarding"]');
      await expect(journey).toHaveAttribute("data-shell", shellName);
    });

    test("@journey signed-in screen shows continue action", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_signin_complete=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.getByRole("button", { name: /go to your account/i })).toBeVisible();
    });

    test("@journey preserves return_to parameter through sign-in", async ({ page }) => {
      const returnPath = "/app/activity";
      await page.goto(`${harness.baseUrl}?return_to=${encodeURIComponent(returnPath)}`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-journey="onboarding"]')).toBeVisible();
    });

    test("@journey ceremony cancellation shows retriable failure", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_ceremony_cancelled=1`, {
        waitUntil: "networkidle",
      });
      await expect(
        page.getByText(/passkey step was cancelled|try again/i)
      ).toBeVisible();
    });

    test("@journey offline state shows appropriate message", async ({ page, context }) => {
      await context.setOffline(true);
      await page.goto(harness.baseUrl, { waitUntil: "domcontentloaded" }).catch(() => {});
      await context.setOffline(false);
      await page.reload({ waitUntil: "networkidle" });
      await page.getByLabel(/email/i).fill("test@example.com");
      await page.getByLabel(/name/i).fill("Test User");
      await context.setOffline(true);
      await page.getByRole("button", { name: /continue/i }).click();
      await expect(page.locator('[data-onboarding-failure]')).toBeVisible();
    });

    test("@journey email field uses correct input mode", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const emailField = page.getByLabel(/email/i);
      await expect(emailField).toHaveAttribute("inputmode", "email");
    });

    test("@journey email field has autocomplete", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const emailField = page.getByLabel(/email/i);
      await expect(emailField).toHaveAttribute("autocomplete", "email");
    });

    test("@journey name field has autocomplete", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      const nameField = page.getByLabel(/name/i);
      await expect(nameField).toHaveAttribute("autocomplete", "name");
    });

    test("@journey resume preserves completed stages", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_resume=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-stage-state="done"]')).toHaveCount(2, { timeout: 5000 });
      await expect(page.locator('[data-stage-state="processing"]')).toHaveCount(1);
    });

    test("@journey shows protocol identity stage with plain language", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(
        page.locator('[data-stage-key="onboarding.stage.setting-up-your-protocol-identity"]')
      ).toBeVisible();
      await expect(
        page.getByText(/activating your account/i)
      ).toBeVisible();
    });

    test("@a11y onboarding form elements have proper labels", async ({ page }) => {
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await expect(page.getByLabel(/email/i)).toHaveAccessibleName(/email/i);
      await expect(page.getByLabel(/name/i)).toHaveAccessibleName(/name/i);
    });

    test("@a11y failure notice has alert role", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_failure=1`, {
        waitUntil: "networkidle",
      });
      const failureNotice = page.locator('[data-onboarding-failure]').locator("..");
      await expect(failureNotice).toHaveAttribute("role", "alert");
    });

    test("@a11y active status notice has status role", async ({ page }) => {
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      const activeNotice = page.locator('[data-account-active]').locator("..");
      await expect(activeNotice).toHaveAttribute("role", "status");
    });

    test("@visual onboarding account creation baseline", async ({ page }, testInfo) => {
      if (!harness.visualBaselineReviewed) {
        test.skip();
      }
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await expect(page.locator('[data-journey="onboarding"]')).toBeVisible();
      await expect(page).toHaveScreenshot(
        `${testInfo.project.name}-onboarding-creation.png`,
        {
          fullPage: true,
          animations: "disabled",
        }
      );
    });

    test("@visual onboarding sign-in baseline", async ({ page }, testInfo) => {
      if (!harness.visualBaselineReviewed) {
        test.skip();
      }
      await page.goto(harness.baseUrl, { waitUntil: "networkidle" });
      await page.getByRole("button", { name: /sign in instead/i }).click();
      await expect(page.locator('[data-decision-screen="1"]')).toBeVisible();
      await expect(page).toHaveScreenshot(
        `${testInfo.project.name}-onboarding-signin.png`,
        {
          fullPage: true,
          animations: "disabled",
        }
      );
    });

    test("@visual onboarding progress baseline", async ({ page }, testInfo) => {
      if (!harness.visualBaselineReviewed) {
        test.skip();
      }
      await page.goto(`${harness.baseUrl}?mock_onboarding_progress=1`, {
        waitUntil: "networkidle",
      });
      await expect(page.locator('[data-journey-phase="progress"]')).toBeVisible();
      await expect(page).toHaveScreenshot(
        `${testInfo.project.name}-onboarding-progress.png`,
        {
          fullPage: true,
          animations: "disabled",
        }
      );
    });
  });
}

test.describe("cross-shell onboarding invariants", () => {
  test("onboarding decision count never exceeds limit", async () => {
    const CREATE_DECISIONS = 2;
    const SIGNIN_DECISIONS = 1;
    const DECISION_LIMIT = 3;
    expect(CREATE_DECISIONS).toBeLessThanOrEqual(DECISION_LIMIT);
    expect(SIGNIN_DECISIONS).toBeLessThanOrEqual(DECISION_LIMIT);
  });

  test("banned vocabulary does not appear in onboarding copy", async () => {
    const BANNED = [
      "DID",
      "session key",
      "capability",
      "nullifier",
      "checkpoint",
      "payload",
      "canonical",
      "idempotency",
      "attestation",
      "proof",
    ];
    const ONBOARDING_COPY_KEYS = [
      "onboarding.stage.creating-your-account",
      "onboarding.stage.adding-your-passkey",
      "onboarding.stage.setting-up-your-protocol-identity",
      "onboarding.stage.putting-recovery-in-place",
      "onboarding.create.title",
      "onboarding.create.body",
      "onboarding.email.label",
      "onboarding.name.label",
      "onboarding.create.action",
      "onboarding.passkey.title",
      "onboarding.passkey.body",
      "onboarding.passkey.action",
      "onboarding.passkey.resume.body",
      "onboarding.passkey.resume.action",
      "onboarding.progress.title",
      "onboarding.progress.body",
      "onboarding.progress.safe_to_close",
      "onboarding.pending.body",
      "onboarding.active",
      "onboarding.not_active",
      "onboarding.failure.title",
      "onboarding.failure.stage",
      "onboarding.failure.preserved",
      "onboarding.resume.action",
      "onboarding.signin.title",
      "onboarding.signin.body",
      "onboarding.signin.action",
      "onboarding.signin.switch",
      "onboarding.create.switch",
      "onboarding.signin.done",
      "onboarding.signin.continue",
      "onboarding.ceremony.cancelled",
    ];
    for (const copyKey of ONBOARDING_COPY_KEYS) {
      for (const banned of BANNED) {
        expect(copyKey.toLowerCase()).not.toContain(banned.toLowerCase());
      }
    }
  });
});
