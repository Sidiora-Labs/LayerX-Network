import type { BrowserContext, Locator } from '@playwright/test';
import React from 'react';

import * as rewardsBalanceMock from 'mocks/rewards/balance';
import * as dailyRewardMock from 'mocks/rewards/dailyReward';
import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { contextWithRewards } from 'playwright/fixtures/rewards';
import { test, expect } from 'playwright/lib';

import NavigationDesktop from './NavigationDesktop';

const testWithAuth = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
}).extend<{ context: BrowserContext }>({
  context: contextWithRewards,
});

testWithAuth('base view +@dark-mode', async({ render, mockApiResponse, mockEnvs, page }) => {
  const hooksConfig = {
    router: {
      route: '/blocks',
      pathname: '/blocks',
    },
  };

  await mockApiResponse('general:user_info', profileMock.withEmailAndWallet);
  await mockApiResponse('rewards:user_balances', rewardsBalanceMock.base);
  await mockApiResponse('rewards:user_daily_check', dailyRewardMock.base);
  await mockEnvs([
    ...ENVS_MAP.userOps,
    ...ENVS_MAP.nameService,
    ...ENVS_MAP.rewardsService,
    [ 'NEXT_PUBLIC_NAVIGATION_HIGHLIGHTED_ROUTES', '["/blocks","/apps"]' ],
  ]);

  const component = await render(<NavigationDesktop/>, { hooksConfig });
  await component.getByText('Blockchain').hover();
  await expect(page.getByText('Blocks')).toBeVisible();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1500, height: 450 } });
});

test('with groped items', async({ render, mockEnvs, page }) => {
  const hooksConfig = {
    router: {
      route: '/apps',
      pathname: '/apps',
    },
  };

  await mockEnvs([
    ...ENVS_MAP.optimisticRollup,
    ...ENVS_MAP.userOps,
    ...ENVS_MAP.nameService,
    ...ENVS_MAP.navigationHighlightedRoutes,
    ...ENVS_MAP.noWalletClient,
    ...ENVS_MAP.noAccount,
  ]);

  const component = await render(<NavigationDesktop/>, { hooksConfig });
  await component.getByText('Blockchain').hover();
  await expect(page.getByText('Blocks')).toBeVisible();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1500, height: 450 } });
});

const promoBannerTest = (type: 'text' | 'image') => {
  test.describe(`with promo banner (${ type })`, () => {
    let component: Locator;
    const darkModeRule = type === 'text' ? '+@dark-mode' : '';
    const imageAltText = type === 'text' ? 'Promo banner icon' : 'Promo banner small';

    test.beforeEach(async({ render, mockEnvs, mockAssetResponse }) => {
      await mockEnvs([
        [ 'NEXT_PUBLIC_NAVIGATION_LAYOUT', 'horizontal' ],
        ...(type === 'text' ? ENVS_MAP.navigationPromoBannerText : ENVS_MAP.navigationPromoBannerImage),
      ]);
      await mockAssetResponse('http://localhost:3000/image.svg', './playwright/mocks/image_svg.svg');
      await mockAssetResponse('http://localhost:3000/image_s.jpg', './playwright/mocks/image_s.jpg');
      await mockAssetResponse('http://localhost:3000/image_md.jpg', './playwright/mocks/image_md.jpg');

      component = await render(<NavigationDesktop/>);
      await component.waitFor({ state: 'visible' });
    });

    test(`${ darkModeRule }`, async() => {
      await expect(component).toHaveScreenshot();
    });

    test('with tooltip', async({ page }) => {
      await page.getByAltText(imageAltText).hover();
      await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1500, height: 450 } });
    });
  });
};

promoBannerTest('text');
promoBannerTest('image');
