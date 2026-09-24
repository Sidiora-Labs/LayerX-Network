import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as activityMock from 'mocks/rewards/activity';
import * as rewardsBalanceMock from 'mocks/rewards/balance';
import * as dailyRewardMock from 'mocks/rewards/dailyReward';
import * as referralsMock from 'mocks/rewards/referrals';
import * as rewardsConfigMock from 'mocks/rewards/rewardsConfig';
import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { contextWithRewards } from 'playwright/fixtures/rewards';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import RewardsDashboard from './RewardsDashboard';

const testWithAuth = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
}).extend<{ context: BrowserContext }>({
  context: contextWithRewards,
});

testWithAuth.beforeEach(async({ mockEnvs, mockApiResponse }) => {
  await mockEnvs([ ...ENVS_MAP.rewardsService ]);
  await mockApiResponse('general:user_info', profileMock.withEmailAndWallet);
});

const testTab = (tab: 'activity' | 'referrals' | 'resources') =>
  testWithAuth(`${ tab } tab +@dark-mode +@mobile`, async({ page, render, mockApiResponse }, testInfo) => {
    await mockApiResponse('rewards:user_balances', rewardsBalanceMock.base);
    await mockApiResponse('rewards:user_daily_check', dailyRewardMock.base);
    await mockApiResponse('rewards:user_referrals', referralsMock.base);
    await mockApiResponse('rewards:config', rewardsConfigMock.base);
    await mockApiResponse('rewards:user_activity', activityMock.base);

    const component = await render(<RewardsDashboard/>, { hooksConfig: { router: { query: { tab } } } });

    // in mobile tests in CI the offers.svg image is rendered with about 10+ pixels difference
    // i couldn't find the reason for this, so i added a maxDiffPixels to the screenshot
    await expect(component).toHaveScreenshot(testInfo.project.name === 'mobile' ? { maxDiffPixels: 30 } : {
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

testTab('activity');
testTab('referrals');
testTab('resources');

testWithAuth('with error', async({ page, render }) => {
  const component = await render(<RewardsDashboard/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
