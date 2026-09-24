import React from 'react';

import * as scrollTxnBatchesMock from 'mocks/scroll/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ScrollL2TxnBatches from './ScrollL2TxnBatches';

test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.scrollRollup);
  await mockTextAd();
  await mockApiResponse('general:scroll_l2_txn_batches', scrollTxnBatchesMock.baseResponse);
  await mockApiResponse('general:scroll_l2_txn_batches_count', 9927);

  const component = await render(<ScrollL2TxnBatches/>);
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
    test.slow();
    await mockEnvs(ENVS_MAP.scrollRollup);
    await mockTextAd();
    await mockApiResponse('general:scroll_l2_txn_batches', scrollTxnBatchesMock.baseResponse);
    await mockApiResponse('general:scroll_l2_txn_batches_count', 9927);

    const component = await render(<ScrollL2TxnBatches/>);
    await expect(component).toHaveScreenshot();
  });
});
