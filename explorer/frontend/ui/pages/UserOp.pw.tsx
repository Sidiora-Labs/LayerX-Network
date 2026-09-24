import React from 'react';

import { userOpData } from 'mocks/userOps/userOp';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import UserOp from './UserOp';

const hooksConfig = {
  router: {
    query: { hash: userOpData.hash },
    isReady: true,
  },
};

test.beforeEach(async({ mockEnvs }) => {
  await mockEnvs(ENVS_MAP.userOps);
});

test('base view', async({ render, mockTextAd, mockApiResponse }) => {
  await mockTextAd();
  await mockApiResponse('general:user_op', userOpData, { pathParams: { hash: userOpData.hash } });
  const component = await render(<UserOp/>, { hooksConfig });
  await component.getByText('View details').click();
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockTextAd, mockApiResponse }) => {
    await mockTextAd();
    await mockApiResponse('general:user_op', userOpData, { pathParams: { hash: userOpData.hash } });
    const component = await render(<UserOp/>, { hooksConfig });
    await component.getByText('View details').click();
    await expect(component).toHaveScreenshot();
  });
});
