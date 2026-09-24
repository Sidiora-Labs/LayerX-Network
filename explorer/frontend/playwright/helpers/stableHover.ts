import type { Locator } from '@playwright/test';

import { expect } from 'playwright/lib';

export async function stableHover(element: Locator) {
  await expect(element).toBeVisible();
  await expect(element).toBeAttached();

  const page = element.page();
  const box = await element.boundingBox();

  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.waitForTimeout(50);
}
