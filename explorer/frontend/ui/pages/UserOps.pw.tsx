import React from 'react';

import { userOpsData } from 'mocks/userOps/userOps';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import UserOps from './UserOps';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.userOps);
  await mockTextAd();
  await mockApiResponse('general:user_ops', userOpsData);
  const component = await render(<UserOps/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
