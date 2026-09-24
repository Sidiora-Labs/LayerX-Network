import React from 'react';

import * as depositMock from 'mocks/optimism/deposits';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import LatestOptimisticDeposits from './LatestOptimisticDeposits';

test('default view +@mobile +@dark-mode', async({ render, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.optimisticRollup);
  mockApiResponse('general:homepage_optimistic_deposits', depositMock.data.items);
  const component = await render(<LatestOptimisticDeposits/>);
  await expect(component).toHaveScreenshot();
});
