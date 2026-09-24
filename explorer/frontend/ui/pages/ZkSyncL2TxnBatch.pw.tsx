import React from 'react';

import * as zkSyncTxnBatchMock from 'mocks/zkSync/zkSyncTxnBatch';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ZkSyncL2TxnBatch from './ZkSyncL2TxnBatch';

const batchNumber = String(zkSyncTxnBatchMock.base.number);
const hooksConfig = {
  router: {
    query: { number: batchNumber },
  },
};

test.beforeEach(async({ mockTextAd, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.zkSyncRollup);
  await mockTextAd();
  await mockApiResponse('general:zksync_l2_txn_batch', zkSyncTxnBatchMock.base, { pathParams: { number: batchNumber } });
});

test('base view', async({ render }) => {
  const component = await render(<ZkSyncL2TxnBatch/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', async({ render }) => {
    const component = await render(<ZkSyncL2TxnBatch/>, { hooksConfig });
    await expect(component).toHaveScreenshot();
  });
});
