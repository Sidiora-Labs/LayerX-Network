import React from 'react';

import { txnBatchesData } from 'mocks/zkEvm/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import LatestZkEvmL2Batches from './LatestZkEvmL2Batches';

test('default view +@mobile +@dark-mode', async({ render, mockEnvs, mockApiResponse }) => {
  await mockEnvs(ENVS_MAP.zkEvmRollup);
  await mockApiResponse('general:homepage_zkevm_l2_batches', txnBatchesData);

  const component = await render(<LatestZkEvmL2Batches/>);
  await expect(component).toHaveScreenshot();
});
