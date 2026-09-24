import React from 'react';

import { outputRootsData } from 'mocks/optimism/outputRoots';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2OutputRoots from './OptimisticL2OutputRoots';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  // test on mobile is flaky
  // my assumption is there is not enough time to calculate hashes truncation so component is unstable
  // so I raised the test timeout to check if it helps
  test.slow();
  await mockEnvs(ENVS_MAP.optimisticRollup);
  await mockTextAd();
  await mockApiResponse('general:optimistic_l2_output_roots', outputRootsData);
  await mockApiResponse('general:optimistic_l2_output_roots_count', 9927);
  const component = await render(<OptimisticL2OutputRoots/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
