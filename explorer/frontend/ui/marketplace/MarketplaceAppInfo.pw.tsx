import React from 'react';

import { apps as appsMock } from 'mocks/apps/apps';
import { test, expect, devices } from 'playwright/lib';

import MarketplaceAppInfo from './MarketplaceAppInfo';

test('base view +@dark-mode', async({ render, page }) => {
  await render(<MarketplaceAppInfo data={ appsMock[0] }/>);
  await page.getByText('Info').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 500, height: 400 } });
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, page }) => {
    await render(<MarketplaceAppInfo data={ appsMock[0] }/>);
    await page.getByLabel('Show info').click();
    await expect(page).toHaveScreenshot();
  });
});
