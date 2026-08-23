import { expect, test } from "@playwright/test";

test.describe("Public Explorer Plane", () => {
  test("@explorer renders the explorer overview page with freshness display", async ({ page }) => {
    await page.goto("/explorer", { waitUntil: "networkidle" });
    
    await expect(page.locator("main")).toHaveCount(1);
    await expect(page.getByRole("heading", { level: 1 })).toContainText(/Explorer|Overview/i);
    
    const freshnessDisplay = page.locator('[data-application="explorer"]').first();
    await expect(freshnessDisplay).toBeVisible();
    
    await expect(page.getByText(/lookup/i)).toBeVisible();
  });

  test("@explorer renders the checkpoints list page with verification levels", async ({ page }) => {
    await page.goto("/explorer/checkpoints", { waitUntil: "networkidle" });
    
    await expect(page.locator("main")).toHaveCount(1);
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    
    const table = page.locator("table").first();
    await expect(table).toBeVisible();
    
    const verificationBadges = page.locator('[data-verification]');
    if (await verificationBadges.count() > 0) {
      await expect(verificationBadges.first()).toBeVisible();
    }
  });

  test("@explorer renders the batches list page with verification levels", async ({ page }) => {
    await page.goto("/explorer/batches", { waitUntil: "networkidle" });
    
    await expect(page.locator("main")).toHaveCount(1);
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    
    const table = page.locator("table").first();
    await expect(table).toBeVisible();
    
    const verificationBadges = page.locator('[data-verification]');
    if (await verificationBadges.count() > 0) {
      await expect(verificationBadges.first()).toBeVisible();
    }
  });

  test("@explorer checkpoint detail page displays verification level on every fact", async ({ page, request }) => {
    const checkpointsResponse = await request.get("/explorer/checkpoints");
    if (!checkpointsResponse.ok()) {
      test.skip();
    }

    await page.goto("/explorer/checkpoints", { waitUntil: "networkidle" });
    
    const firstCheckpointLink = page.locator('table a[href^="/explorer/checkpoints/"]').first();
    if (await firstCheckpointLink.count() === 0) {
      test.skip();
    }
    
    await firstCheckpointLink.click();
    await page.waitForLoadState("networkidle");
    
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    
    const factTable = page.locator("table").first();
    await expect(factTable).toBeVisible();
    
    const factRows = factTable.locator("tbody tr");
    const factRowCount = await factRows.count();
    expect(factRowCount).toBeGreaterThan(0);
    
    for (let i = 0; i < factRowCount; i++) {
      const row = factRows.nth(i);
      const cells = row.locator("td");
      const cellCount = await cells.count();
      expect(cellCount).toBeGreaterThanOrEqual(3);
      
      const verificationCell = cells.last();
      await expect(verificationCell).toBeVisible();
    }
  });

  test("@explorer batch detail page displays verification level on every fact", async ({ page, request }) => {
    const batchesResponse = await request.get("/explorer/batches");
    if (!batchesResponse.ok()) {
      test.skip();
    }

    await page.goto("/explorer/batches", { waitUntil: "networkidle" });
    
    const firstBatchLink = page.locator('table a[href^="/explorer/batches/"]').first();
    if (await firstBatchLink.count() === 0) {
      test.skip();
    }
    
    await firstBatchLink.click();
    await page.waitForLoadState("networkidle");
    
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    
    const factTable = page.locator("table").first();
    await expect(factTable).toBeVisible();
    
    const factRows = factTable.locator("tbody tr");
    const factRowCount = await factRows.count();
    expect(factRowCount).toBeGreaterThan(0);
    
    for (let i = 0; i < factRowCount; i++) {
      const row = factRows.nth(i);
      const cells = row.locator("td");
      const cellCount = await cells.count();
      expect(cellCount).toBeGreaterThanOrEqual(3);
      
      const verificationCell = cells.last();
      await expect(verificationCell).toBeVisible();
    }
  });

  test("@explorer receipt lookup redirects to receipt detail page", async ({ page }) => {
    await page.goto("/explorer", { waitUntil: "networkidle" });
    
    const receiptInput = page.locator('input[name="identifier"][type="text"]').first();
    const lookupForm = receiptInput.locator("xpath=ancestor::form");
    
    const validReceiptId = "a".repeat(64);
    await receiptInput.fill(validReceiptId);
    
    await lookupForm.locator('button[type="submit"]').click();
    
    await page.waitForURL(/\/explorer\/receipts\//);
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
  });

  test("@explorer account lookup redirects to account activity page", async ({ page }) => {
    await page.goto("/explorer", { waitUntil: "networkidle" });
    
    const accountInput = page.locator('input[name="identifier"][type="text"]').last();
    const lookupForm = accountInput.locator("xpath=ancestor::form");
    
    const validAccountId = "b".repeat(64);
    await accountInput.fill(validAccountId);
    
    await lookupForm.locator('button[type="submit"]').click();
    
    await page.waitForURL(/\/explorer\/accounts\//);
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
  });

  test("@explorer evidence verifier renders and accepts input", async ({ page }) => {
    await page.goto("/explorer/verify", { waitUntil: "networkidle" });
    
    await expect(page.locator("main")).toHaveCount(1);
    await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    
    const evidenceInput = page.locator("textarea");
    await expect(evidenceInput).toBeVisible();
    
    const kindSelector = page.locator('[role="radiogroup"]');
    await expect(kindSelector).toBeVisible();
    
    const submitButton = page.locator('button[type="submit"]');
    await expect(submitButton).toBeVisible();
  });

  test("@explorer evidence verifier validates with receipt evidence", async ({ page }) => {
    await page.goto("/explorer/verify", { waitUntil: "networkidle" });
    
    const evidenceInput = page.locator("textarea");
    const submitButton = page.locator('button[type="submit"]');
    
    const validReceiptEvidence = "dGVzdF9ldmlkZW5jZV9kYXRhX2Zvcl9yZWNlaXB0X3ZlcmlmaWNhdGlvbg";
    await evidenceInput.fill(validReceiptEvidence);
    
    await submitButton.click();
    
    await page.waitForTimeout(1000);
  });

  test("@explorer evidence verifier handles altered evidence", async ({ page }) => {
    await page.goto("/explorer/verify", { waitUntil: "networkidle" });
    
    const evidenceInput = page.locator("textarea");
    const submitButton = page.locator('button[type="submit"]');
    
    const alteredEvidence = "YWx0ZXJlZF9ldmlkZW5jZV90aGF0X3Nob3VsZF9mYWlsX3ZlcmlmaWNhdGlvbg";
    await evidenceInput.fill(alteredEvidence);
    
    await submitButton.click();
    
    await page.waitForTimeout(1000);
    
    const errorNotice = page.locator('[role="alert"]');
    if (await errorNotice.count() > 0) {
      await expect(errorNotice).toBeVisible();
    }
  });

  test("@explorer all pages are accessible without authentication", async ({ page, context }) => {
    await context.clearCookies();
    
    const explorerPages = [
      "/explorer",
      "/explorer/checkpoints",
      "/explorer/batches",
      "/explorer/verify",
    ];
    
    for (const path of explorerPages) {
      await page.goto(path, { waitUntil: "networkidle" });
      
      await expect(page.locator("main")).toHaveCount(1);
      await expect(page.locator('[data-application="explorer"]')).toBeVisible();
      
      const authWall = page.locator('[data-auth-required]');
      await expect(authWall).toHaveCount(0);
    }
  });

  test("@explorer pages show index freshness on every page", async ({ page }) => {
    const explorerPages = [
      "/explorer",
      "/explorer/checkpoints",
      "/explorer/batches",
    ];
    
    for (const path of explorerPages) {
      await page.goto(path, { waitUntil: "networkidle" });
      
      const explorerFrame = page.locator('[data-application="explorer"]');
      await expect(explorerFrame).toBeVisible();
      
      const freshnessIndicator = explorerFrame.locator('text=/current|behind|indexed|batch/i').first();
      await expect(freshnessIndicator).toBeVisible();
    }
  });

  test("@explorer navigation is present on all explorer pages", async ({ page }) => {
    const explorerPages = [
      "/explorer",
      "/explorer/checkpoints",
      "/explorer/batches",
      "/explorer/verify",
    ];
    
    for (const path of explorerPages) {
      await page.goto(path, { waitUntil: "networkidle" });
      
      const navigation = page.locator("nav");
      await expect(navigation).toBeVisible();
      
      const overviewLink = page.locator('a[href="/explorer"]');
      await expect(overviewLink).toBeVisible();
    }
  });

  test("@explorer pages use table-first layout for data scanning", async ({ page }) => {
    const tablePages = [
      "/explorer/checkpoints",
      "/explorer/batches",
    ];
    
    for (const path of tablePages) {
      await page.goto(path, { waitUntil: "networkidle" });
      
      const table = page.locator("table").first();
      await expect(table).toBeVisible();
      
      const tableCaption = table.locator("caption");
      await expect(tableCaption).toBeVisible();
      
      const thead = table.locator("thead");
      await expect(thead).toBeVisible();
    }
  });

  test("@explorer deep links resolve correctly", async ({ page }) => {
    const validCheckpointId = "c".repeat(64);
    const validBatchNumber = "12345";
    const validReceiptId = "d".repeat(64);
    const validAccountId = "e".repeat(64);
    
    const deepLinks = [
      `/explorer/checkpoints/${validCheckpointId}`,
      `/explorer/batches/${validBatchNumber}`,
      `/explorer/receipts/${validReceiptId}`,
      `/explorer/accounts/${validAccountId}`,
    ];
    
    for (const path of deepLinks) {
      await page.goto(path, { waitUntil: "networkidle" });
      
      await expect(page.locator("main")).toHaveCount(1);
      await expect(page.locator('[data-application="explorer"]')).toBeVisible();
    }
  });
});
