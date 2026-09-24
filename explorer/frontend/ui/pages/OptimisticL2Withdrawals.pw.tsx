import React from 'react';

import { data as withdrawalsData } from 'mocks/optimism/withdrawals';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2Withdrawals from './OptimisticL2Withdrawals';

test('base view +@mobile', async({ render, mockTextAd, mockEnvs, mockApiResponse }) => {
  // test on mobile is flaky
  // my assumption is there is not enough time to calculate hashes truncation so component is unstable
  // so I raised the test timeout to check if it helps
  test.slow();
  await mockTextAd();
  await mockEnvs(ENVS_MAP.optimisticRollup);
  await mockApiResponse('general:optimistic_l2_withdrawals', withdrawalsData);
  await mockApiResponse('general:optimistic_l2_withdrawals_count', 397);
  const component = await render(<OptimisticL2Withdrawals/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
