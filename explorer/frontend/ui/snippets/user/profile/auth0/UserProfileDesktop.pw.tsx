import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { test as base, expect } from 'playwright/lib';

import UserProfileDesktop from './UserProfileDesktop';

const test = base.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

test('without address', async({ render, page, mockApiResponse }) => {
  await mockApiResponse('general:user_info', profileMock.base);
  await render(<UserProfileDesktop/>, undefined, { marketplaceContext: { isAutoConnectDisabled: true, setIsAutoConnectDisabled: () => {} } });
  await page.getByText(/tom/i).click();

  await expect(page).toHaveScreenshot({
    clip: { x: 0, y: 0, width: 300, height: 700 },
  });
});

test('without email', async({ render, page, mockApiResponse }) => {
  await mockApiResponse('general:user_info', profileMock.withoutEmail);
  await render(<UserProfileDesktop/>);
  await page.getByText(/my profile/i).click();

  await expect(page).toHaveScreenshot({
    clip: { x: 0, y: 0, width: 300, height: 600 },
  });
});
