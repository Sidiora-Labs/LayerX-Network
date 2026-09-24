import React from 'react';

import { finalized, unfinalized } from 'mocks/arbitrum/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import LatestArbitrumL2Batches from './LatestArbitrumL2Batches';

test('default view +@mobile +@dark-mode', async({ render, mockEnvs, mockApiResponse }) => {
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  await mockApiResponse('general:homepage_arbitrum_l2_batches', { items: [ finalized, unfinalized ] });

  const component = await render(<LatestArbitrumL2Batches/>);
  await expect(component).toHaveScreenshot();
});
