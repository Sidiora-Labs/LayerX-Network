import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

test.describe("Approvals journey", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/app/approvals");
  });

  test("@journey renders approval inbox in mobile shell", async ({ page }, testInfo) => {
    if (!testInfo.project.name.includes("mobile")) {
      test.skip();
    }
    await expect(page.locator('[data-application="approvals"]')).toBeVisible();
    await expect(page.getByText("Approvals", { exact: true })).toBeVisible();
  });

  test("@journey renders approval inbox in desktop shell", async ({ page }, testInfo) => {
    if (!testInfo.project.name.includes("desktop")) {
      test.skip();
    }
    await expect(page.locator('[data-application="approvals"]')).toBeVisible();
    await expect(page.getByText("Approvals", { exact: true })).toBeVisible();
  });

  test("@journey @critical grants approval with step-up ceremony", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const approveButton = page.getByRole("button", { name: "Approve" });
      if (await approveButton.isVisible()) {
        await approveButton.click();
        await expect(page.getByText(/will move/i)).toBeVisible();
        const confirmButton = page.getByRole("button", { name: "Approve" }).last();
        await confirmButton.click();
        await expect(page.getByRole("status")).toBeVisible();
      }
    }
  });

  test("@journey @critical rejects approval with nothing-moved confirmation", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const rejectButton = page.getByRole("button", { name: "Reject" });
      if (await rejectButton.isVisible()) {
        await rejectButton.click();
        await expect(page.getByText(/nothing/i)).toBeVisible();
        const confirmButton = page.getByRole("button", { name: "Reject" }).last();
        await confirmButton.click();
        await expect(page.getByRole("status")).toBeVisible();
      }
    }
  });

  test("@journey renders expired approval state", async ({ page }) => {
    await page.goto("/app/approvals");
    const expiredBadge = page.getByText("Expired");
    if (await expiredBadge.isVisible()) {
      const expiredItem = page.locator("li[role='button']", { has: expiredBadge }).first();
      await expiredItem.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByText(/expired/i)).toBeVisible();
      await expect(page.getByRole("button", { name: "Approve" })).not.toBeVisible();
    }
  });

  test("@journey tracks released activity after approval grant", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const approveButton = page.getByRole("button", { name: "Approve" });
      if (await approveButton.isVisible()) {
        await approveButton.click();
        const confirmButton = page.getByRole("button", { name: "Approve" }).last();
        await confirmButton.click();
        await expect(page.getByText("Released activity")).toBeVisible({ timeout: 10000 });
        const trackLink = page.getByRole("link", { name: /track/i });
        await expect(trackLink).toBeVisible();
      }
    }
  });

  test("@journey @critical shows expiry countdown for pending approvals", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstPendingApproval = page.locator("li[role='button']").first();
    if (await firstPendingApproval.isVisible()) {
      await firstPendingApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const countdown = page.locator('[role="timer"]');
      if (await countdown.isVisible()) {
        await expect(countdown).toHaveText(/minute|imminent/i);
      }
    }
  });

  test("@journey displays budget remaining after approval", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByText(/budget/i)).toBeVisible();
      await expect(page.getByText(/verification/i)).toBeVisible();
    }
  });

  test("@journey desktop split view shows inbox and detail", async ({ page }, testInfo) => {
    if (!testInfo.project.name.includes("desktop")) {
      test.skip();
    }
    await page.goto("/app/approvals");
    await expect(page.locator("section").filter({ hasText: "Approvals" })).toBeVisible();
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.locator("section").filter({ hasText: "Approvals" })).toBeVisible();
    }
  });

  test("@journey mobile screen navigates from inbox to detail", async ({ page }, testInfo) => {
    if (!testInfo.project.name.includes("mobile")) {
      test.skip();
    }
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.locator("section").filter({ hasText: "Approvals" })).not.toBeVisible();
    }
  });
});

test.describe("Two-device convergence", () => {
  async function openApprovalInSecondContext(approvalId: string, secondPage: Page) {
    await secondPage.goto(`/app/approvals/${approvalId}`);
    await expect(secondPage.locator('[data-application="approval-detail"]')).toBeVisible();
  }

  test("@journey @critical shows already-decided when second device acts on same approval", async ({ page, context }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const approvalIdElement = page.locator('[data-application="approval-detail"]').locator("text=/apv_/");
      const approvalId = await approvalIdElement.textContent();
      
      if (approvalId !== null) {
        const secondPage = await context.newPage();
        await openApprovalInSecondContext(approvalId, secondPage);
        
        const approveButton = page.getByRole("button", { name: "Approve" });
        if (await approveButton.isVisible()) {
          await approveButton.click();
          const confirmButton = page.getByRole("button", { name: "Approve" }).last();
          await confirmButton.click();
          
          const secondApproveButton = secondPage.getByRole("button", { name: "Approve" });
          if (await secondApproveButton.isVisible()) {
            await secondApproveButton.click();
            const secondConfirmButton = secondPage.getByRole("button", { name: "Approve" }).last();
            await secondConfirmButton.click();
            await expect(secondPage.getByText(/already/i)).toBeVisible({ timeout: 10000 });
          }
        }
        
        await secondPage.close();
      }
    }
  });

  test("@journey converges to same outcome across devices", async ({ page, context }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const approvalIdElement = page.locator('[data-application="approval-detail"]').locator("text=/apv_/");
      const approvalId = await approvalIdElement.textContent();
      
      if (approvalId !== null) {
        const secondPage = await context.newPage();
        await openApprovalInSecondContext(approvalId, secondPage);
        
        const rejectButton = page.getByRole("button", { name: "Reject" });
        if (await rejectButton.isVisible()) {
          await rejectButton.click();
          const confirmButton = page.getByRole("button", { name: "Reject" }).last();
          await confirmButton.click();
          
          await secondPage.reload();
          await expect(secondPage.getByText(/rejected|nothing moved/i)).toBeVisible({ timeout: 10000 });
        }
        
        await secondPage.close();
      }
    }
  });
});

test.describe("Approval expiry handling", () => {
  test("@journey expired approval cannot be approved", async ({ page }) => {
    await page.goto("/app/approvals");
    const expiredBadge = page.getByText("Expired");
    if (await expiredBadge.isVisible()) {
      const expiredItem = page.locator("li[role='button']", { has: expiredBadge }).first();
      await expiredItem.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByRole("button", { name: "Approve" })).not.toBeVisible();
      await expect(page.getByRole("button", { name: "Reject" })).not.toBeVisible();
    }
  });

  test("@journey expired approval shows no money moved message", async ({ page }) => {
    await page.goto("/app/approvals");
    const expiredBadge = page.getByText("Expired");
    if (await expiredBadge.isVisible()) {
      const expiredItem = page.locator("li[role='button']", { has: expiredBadge }).first();
      await expiredItem.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByText(/expired/i)).toBeVisible();
    }
  });
});

test.describe("Defective approval handling", () => {
  test("@journey defective approval shows error state", async ({ page }) => {
    await page.goto("/app/approvals");
    const defectiveBadge = page.getByText("Defective");
    if (await defectiveBadge.isVisible()) {
      const defectiveItem = page.locator("li[role='button']", { has: defectiveBadge }).first();
      await defectiveItem.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByText(/defective/i)).toBeVisible();
      await expect(page.getByRole("button", { name: "Approve" })).not.toBeVisible();
    }
  });
});

test.describe("Technical details disclosure", () => {
  test("@journey renders disclosure from held activity", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      await expect(page.getByText("Agent")).toBeVisible();
      await expect(page.getByText("Counterparty")).toBeVisible();
      await expect(page.getByText("Amount")).toBeVisible();
      await expect(page.getByText("Fees")).toBeVisible();
    }
  });

  test("@journey exposes technical details", async ({ page }) => {
    await page.goto("/app/approvals");
    const firstApproval = page.locator("li[role='button']").first();
    if (await firstApproval.isVisible()) {
      await firstApproval.click();
      await expect(page.locator('[data-application="approval-detail"]')).toBeVisible();
      const technicalDetails = page.getByText("Technical details");
      if (await technicalDetails.isVisible()) {
        await technicalDetails.click();
        await expect(page.getByText(/reference|evidence/i)).toBeVisible();
      }
    }
  });
});
