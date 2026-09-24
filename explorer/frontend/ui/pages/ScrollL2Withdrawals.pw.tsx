import React from 'react';

import * as messagesMock from 'mocks/scroll/messages';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ScrollL2Withdrawals from './ScrollL2Withdrawals';

test('base view', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockTextAd();
  await mockEnvs(ENVS_MAP.scrollRollup);
  await mockApiResponse('general:scroll_l2_withdrawals', messagesMock.baseResponse);
  await mockApiResponse('general:scroll_l2_withdrawals_count', 3971111);

  const component = await render(<ScrollL2Withdrawals/>);

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
    await mockTextAd();
    await mockEnvs(ENVS_MAP.scrollRollup);
    await mockApiResponse('general:scroll_l2_withdrawals', messagesMock.baseResponse);
    await mockApiResponse('general:scroll_l2_withdrawals_count', 3971111);

    const component = await render(<ScrollL2Withdrawals/>);

    await expect(component).toHaveScreenshot();
  });
});
