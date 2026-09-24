import React from 'react';

import { data as depositsData } from 'mocks/optimism/deposits';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2Deposits from './OptimisticL2Deposits';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  // test on mobile is flaky
  // my assumption is there is not enough time to calculate hashes truncation so component is unstable
  // so I raised the test timeout to check if it helps
  test.slow();
  await mockEnvs(ENVS_MAP.optimisticRollup);
  await mockTextAd();
  await mockApiResponse('general:optimistic_l2_deposits', depositsData);
  await mockApiResponse('general:optimistic_l2_deposits_count', 3971111);

  const component = await render(<OptimisticL2Deposits/>);

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
