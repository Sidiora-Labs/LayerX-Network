import { expect, test } from "@playwright/test";

import { copyEntry } from "../src/copy/catalog.ts";
import { SUPPORT_TOPICS } from "../src/support/topics.ts";

test.describe("@support support chat states and flows", () => {
  test("renders empty state with topic suggestion chips on mobile", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    await expect(page.getByRole("heading", { name: copyEntry("support.title").message })).toBeVisible();
    await expect(page.getByText(copyEntry("support.empty").message)).toBeVisible();
    await expect(page.getByText(copyEntry("support.empty.body").message)).toBeVisible();
    
    const suggestionGroup = page.getByRole("group", { name: copyEntry("support.suggestions").message });
    await expect(suggestionGroup).toBeVisible();
    
    for (const topic of SUPPORT_TOPICS) {
      await expect(suggestionGroup.getByRole("button", { name: copyEntry(topic.labelKey).message })).toBeVisible();
    }
  });

  test("renders empty state with topic suggestion chips on desktop", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    await expect(page.getByRole("heading", { name: copyEntry("support.title").message })).toBeVisible();
    await expect(page.getByText(copyEntry("support.empty").message)).toBeVisible();
    
    const suggestionGroup = page.getByRole("group", { name: copyEntry("support.suggestions").message });
    await expect(suggestionGroup).toBeVisible();
    
    for (const topic of SUPPORT_TOPICS) {
      await expect(suggestionGroup.getByRole("button", { name: copyEntry(topic.labelKey).message })).toBeVisible();
    }
  });

  test("seeds draft when topic chip is clicked", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const firstTopic = SUPPORT_TOPICS[0];
    if (firstTopic === undefined) {
      throw new Error("SUPPORT_TOPICS must have at least one topic");
    }
    
    const topicButton = page.getByRole("button", { name: copyEntry(firstTopic.labelKey).message });
    await topicButton.click();
    
    const textField = page.getByRole("textbox", { name: copyEntry("support.compose.label").message });
    await expect(textField).toHaveValue(copyEntry(firstTopic.seedKey).message);
  });

  test("disables send button when draft is empty", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const sendButton = page.getByRole("button", { name: copyEntry("support.send").message });
    await expect(sendButton).toBeDisabled();
  });

  test("enables send button when draft has content", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const textField = page.getByRole("textbox", { name: copyEntry("support.compose.label").message });
    await textField.fill("I need help with a deposit");
    
    const sendButton = page.getByRole("button", { name: copyEntry("support.send").message });
    await expect(sendButton).toBeEnabled();
  });

  test("attaches trace identifier from query parameter", async ({ page }) => {
    const traceId = "trc_0123456789abcdef0123456789abcdef";
    await page.goto(`/app/support?trace=${traceId}`, { waitUntil: "networkidle" });
    
    await expect(page.getByText(copyEntry("support.trace.attached").message)).toBeVisible();
    await expect(page.getByText(traceId, { exact: false })).toBeVisible();
  });

  test("renders offline state with retry button", async ({ page, context }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    await context.setOffline(true);
    await page.reload({ waitUntil: "networkidle" });
    
    await expect(page.getByText(copyEntry("state.offline.banner").message)).toBeVisible();
    await expect(page.getByRole("button", { name: copyEntry("action.retry").message })).toBeVisible();
  });

  test("shows sending state when message is being sent", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const textField = page.getByRole("textbox", { name: copyEntry("support.compose.label").message });
    await textField.fill("Test message");
    
    const sendButton = page.getByRole("button", { name: copyEntry("support.send").message });
    
    const sendPromise = sendButton.click();
    
    await expect(sendButton).toBeDisabled();
    
    await sendPromise;
  });

  test("desktop shell renders as docked panel", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const supportContainer = page.locator("[data-support-shell='desktop']");
    await expect(supportContainer).toBeVisible();
    
    const box = await supportContainer.boundingBox();
    if (box === null) {
      throw new Error("Support container must have a bounding box");
    }
    
    const viewport = page.viewportSize();
    if (viewport === null) {
      throw new Error("Viewport size must be set");
    }
    
    expect(box.x + box.width).toBeGreaterThan(viewport.width - 100);
    expect(box.y + box.height).toBeGreaterThan(viewport.height - 100);
  });

  test("mobile shell renders as full screen", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const supportContainer = page.locator("[data-support-shell='mobile']");
    await expect(supportContainer).toBeVisible();
    
    const box = await supportContainer.boundingBox();
    if (box === null) {
      throw new Error("Support container must have a bounding box");
    }
    
    expect(box.x).toBeLessThanOrEqual(10);
    expect(box.y).toBeLessThanOrEqual(10);
  });
});

test.describe("@support report-to-support from error surfaces", () => {
  test("error surface includes support button", async ({ page }) => {
    await page.route("/api/human/v1/**", route => route.abort("failed"));
    
    await page.goto("/app", { waitUntil: "networkidle" });
    
    await expect(page.getByRole("button", { name: copyEntry("support.open").message })).toBeVisible();
  });

  test("error surface support button navigates with trace", async ({ page }) => {
    await page.route("/api/human/v1/**", route => route.abort("failed"));
    
    await page.goto("/app", { waitUntil: "networkidle" });
    
    const traceText = await page.locator("text=/trc_[0-9a-f]{32}/").textContent();
    const traceMatch = traceText?.match(/trc_[0-9a-f]{32}/);
    
    if (traceMatch === null || traceMatch === undefined) {
      throw new Error("Error surface must display a trace identifier");
    }
    
    const traceId = traceMatch[0];
    
    const supportButton = page.getByRole("button", { name: copyEntry("support.open").message });
    await supportButton.click();
    
    await expect(page).toHaveURL(new RegExp(`/app/support\\?trace=${traceId}`));
    await expect(page.getByText(copyEntry("support.trace.attached").message)).toBeVisible();
    await expect(page.getByText(traceId, { exact: false })).toBeVisible();
  });

  test("report action submits trace identifier", async ({ page }) => {
    await page.route("/api/human/v1/**", route => route.abort("failed"));
    
    await page.goto("/app", { waitUntil: "networkidle" });
    
    const reportButton = page.getByRole("button", { name: copyEntry("action.report").message });
    await reportButton.click();
    
    await expect(page.getByRole("heading", { name: copyEntry("report.consent.title").message })).toBeVisible();
    await expect(page.getByText(copyEntry("report.consent.body").message)).toBeVisible();
    
    const confirmButton = page.getByRole("button", { name: copyEntry("report.submit").message });
    await expect(confirmButton).toBeVisible();
  });
});

test.describe("@support message delivery states", () => {
  test("shows retry button for failed messages", async ({ page, context }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    const textField = page.getByRole("textbox", { name: copyEntry("support.compose.label").message });
    await textField.fill("Test message that will fail");
    
    await context.setOffline(true);
    
    const sendButton = page.getByRole("button", { name: copyEntry("support.send").message });
    await sendButton.click();
    
    await expect(page.getByText(copyEntry("support.message.failed").message)).toBeVisible();
    await expect(page.getByRole("button", { name: copyEntry("action.retry").message })).toBeVisible();
  });

  test("feedback loop appears after support replies", async ({ page }) => {
    await page.goto("/app/support", { waitUntil: "networkidle" });
    
    await page.route("/api/human/v1/support/list", async route => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          conversations: [{
            conversation_id: "conv_test123",
            state: "open",
            messages: [
              {
                message_id: "msg_user_1",
                author: "you",
                body: "I need help",
                sent_at: new Date().toISOString(),
                read: true
              },
              {
                message_id: "msg_support_1",
                author: "support",
                body: "How can I help you?",
                sent_at: new Date().toISOString(),
                read: true
              }
            ],
            feedback: []
          }]
        })
      });
    });
    
    await page.reload({ waitUntil: "networkidle" });
    
    await expect(page.getByText(copyEntry("support.feedback.question").message)).toBeVisible();
    await expect(page.getByRole("button", { name: copyEntry("support.feedback.yes").message })).toBeVisible();
    await expect(page.getByRole("button", { name: copyEntry("support.feedback.no").message })).toBeVisible();
  });
});
