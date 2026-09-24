import React from 'react';

import * as messagesMock from 'mocks/scroll/messages';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ScrollL2Deposits from './ScrollL2Deposits';

test('base view', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockTextAd();
  await mockEnvs(ENVS_MAP.scrollRollup);
  await mockApiResponse('general:scroll_l2_deposits', messagesMock.baseResponse);
  await mockApiResponse('general:scroll_l2_deposits_count', 3971111);

  const component = await render(<ScrollL2Deposits/>);

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
    await mockTextAd();
    await mockEnvs(ENVS_MAP.scrollRollup);
    await mockApiResponse('general:scroll_l2_deposits', messagesMock.baseResponse);
    await mockApiResponse('general:scroll_l2_deposits_count', 3971111);

    const component = await render(<ScrollL2Deposits/>);

    await expect(component).toHaveScreenshot();
  });
});
