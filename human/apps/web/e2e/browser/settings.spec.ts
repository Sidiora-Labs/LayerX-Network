import { expect, test } from "@playwright/test";

import { copyEntry } from "../../copy/catalog.ts";
import { formatCopy } from "../../copy/format.ts";
import { APP_SESSION_COOKIE } from "../../src/auth/session.ts";

function requiredEnvironment(name: string): string {
  const value = process.env[name]?.trim();
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is required for authenticated settings qualification`);
  }
  return value;
}

test.beforeEach(async ({ context }) => {
  const baseUrl = new URL(requiredEnvironment("HUMAN_E2E_BASE_URL"));
  await context.addCookies([{
    name: APP_SESSION_COOKIE,
    value: requiredEnvironment("HUMAN_E2E_SESSION_COOKIE"),
    url: baseUrl.origin,
    httpOnly: true,
    secure: true,
    sameSite: "Strict",
  }]);
});

test("@settings settings preferences persist and privacy masks every figure", async ({ page }) => {
  await page.goto("/app/settings", { waitUntil: "networkidle" });

  for (const section of [
    "settings.section.profile",
    "settings.section.security",
    "settings.section.wallet",
    "settings.section.notifications",
    "settings.section.advanced",
    "settings.section.help",
  ]) {
    await expect(page.getByText(copyEntry(section).message, { exact: true })).toBeVisible();
  }

  const pushLabel = copyEntry("settings.notifications.channel.push").message;
  const pushToggle = page.getByRole("switch", {
    name: formatCopy("settings.notifications.channel.toggle", { channel: pushLabel }),
  });
  const pushWasEnabled = await pushToggle.isChecked();
  if (!pushWasEnabled) {
    const channelSaved = page.waitForResponse((response) =>
      response.request().method() === "POST"
        && new URL(response.url()).pathname === "/v1/notifications/preferences"
    );
    await pushToggle.click();
    await expect((await channelSaved).ok()).toBe(true);
  }
  const approvalLabel = copyEntry("settings.notifications.class.approval_waiting").message;
  const approvalToggle = page.getByRole("switch", {
    name: formatCopy("settings.notifications.class.toggle", {
      notification: approvalLabel,
      channel: pushLabel,
    }),
  });
  const approvalWasEnabled = await approvalToggle.isChecked();
  const preferenceSaved = page.waitForResponse((response) =>
    response.request().method() === "POST"
      && new URL(response.url()).pathname === "/v1/notifications/preferences"
  );
  await approvalToggle.click();
  await expect((await preferenceSaved).ok()).toBe(true);
  await expect(approvalToggle).toBeChecked({ checked: !approvalWasEnabled });
  await page.reload({ waitUntil: "networkidle" });
  await expect(page.getByRole("switch", {
    name: formatCopy("settings.notifications.class.toggle", {
      notification: approvalLabel,
      channel: pushLabel,
    }),
  })).toBeChecked({ checked: !approvalWasEnabled });

  const privacyToggle = page.getByRole("switch", {
    name: copyEntry("settings.privacy.toggle").message,
  });
  const privacyWasEnabled = await privacyToggle.isChecked();
  if (!privacyWasEnabled) {
    await privacyToggle.click();
  }
  await expect(privacyToggle).toBeChecked();

  await page.goto("/app", { waitUntil: "networkidle" });
  const privateFigures = page.locator("[data-private-figure]");
  await expect(privateFigures.first()).toBeVisible();
  expect(await privateFigures.count()).toBeGreaterThan(0);
  expect(await privateFigures.evaluateAll((figures) => figures.every(
    (figure) => figure.getAttribute("data-private-figure") === "masked",
  ))).toBe(true);

  await page.reload({ waitUntil: "networkidle" });
  await expect(page.locator("[data-private-figure]").first()).toHaveAttribute(
    "data-private-figure",
    "masked",
  );

  await page.goto("/app/settings", { waitUntil: "networkidle" });
  if (!privacyWasEnabled) {
    await page.getByRole("switch", { name: copyEntry("settings.privacy.toggle").message }).click();
  }
  const restoredApproval = page.getByRole("switch", {
    name: formatCopy("settings.notifications.class.toggle", {
      notification: approvalLabel,
      channel: pushLabel,
    }),
  });
  if ((await restoredApproval.isChecked()) !== approvalWasEnabled) {
    await restoredApproval.click();
  }
  if (!pushWasEnabled) {
    await page.getByRole("switch", {
      name: formatCopy("settings.notifications.channel.toggle", { channel: pushLabel }),
    }).click();
  }
});
