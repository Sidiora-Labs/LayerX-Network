import React from 'react';

import { apps as appsMock } from 'mocks/apps/apps';
import type { TestFnArgs } from 'playwright/lib';
import { test, expect, devices } from 'playwright/lib';

import MarketplaceAppModal from './MarketplaceAppModal';

const props = {
  onClose: () => {},
  onFavoriteClick: () => {},
  showContractList: () => {},
  data: appsMock[0],
  isFavorite: false,
  userRating: undefined,
  rateApp: () => {},
  isRatingSending: false,
  isRatingLoading: false,
  canRate: undefined,
};

const testFn = async({ render, page, mockAssetResponse }: TestFnArgs) => {
  await mockAssetResponse(appsMock[0].logo, './playwright/mocks/image_s.jpg');
  await render(<MarketplaceAppModal { ...props }/>);
  await page.getByText('Launch app').focus();
  await expect(page).toHaveScreenshot();
};

test('base view +@dark-mode', testFn);

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', testFn);
});
