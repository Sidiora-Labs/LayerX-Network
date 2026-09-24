import React from 'react';

import { data as withdrawalsData } from 'mocks/shibarium/withdrawals';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ShibariumWithdrawals from './ShibariumWithdrawals';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  // test on mobile is flaky
  // my assumption is there is not enough time to calculate hashes truncation so component is unstable
  // so I raised the test timeout to check if it helps
  test.slow();

  await mockTextAd();
  await mockEnvs(ENVS_MAP.shibariumRollup);
  await mockApiResponse('general:shibarium_withdrawals', withdrawalsData);
  await mockApiResponse('general:shibarium_withdrawals_count', 397);

  const component = await render(<ShibariumWithdrawals/>);

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
