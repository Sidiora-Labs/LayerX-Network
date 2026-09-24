import React from 'react';

import { batchData } from 'mocks/scroll/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ScrollL2TxnBatch from './ScrollL2TxnBatch';

const batchNumber = '5';
const hooksConfig = {
  router: {
    query: { number: batchNumber },
  },
};

test.beforeEach(async({ mockTextAd, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.scrollRollup);
  await mockTextAd();
});

test('base view', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:scroll_l2_txn_batch', batchData, { pathParams: { number: batchNumber } });
  const component = await render(<ScrollL2TxnBatch/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:scroll_l2_txn_batch', batchData, { pathParams: { number: batchNumber } });
    const component = await render(<ScrollL2TxnBatch/>, { hooksConfig });
    await expect(component).toHaveScreenshot();
  });
});
