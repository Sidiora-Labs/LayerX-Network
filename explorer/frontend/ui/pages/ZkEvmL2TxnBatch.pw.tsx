import React from 'react';

import { txnBatchData } from 'mocks/zkEvm/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ZkEvmL2TxnBatch from './ZkEvmL2TxnBatch';

const batchNumber = '5';
const hooksConfig = {
  router: {
    query: { number: batchNumber },
  },
};

test.beforeEach(async({ mockTextAd, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.zkEvmRollup);
  await mockTextAd();
  await mockApiResponse('general:zkevm_l2_txn_batch', txnBatchData, { pathParams: { number: batchNumber } });
});

test('base view', async({ render }) => {
  test.slow();
  const component = await render(<ZkEvmL2TxnBatch/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', async({ render }) => {
    test.slow();
    const component = await render(<ZkEvmL2TxnBatch/>, { hooksConfig });
    await expect(component).toHaveScreenshot();
  });
});
