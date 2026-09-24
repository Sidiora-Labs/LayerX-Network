import React from 'react';

import * as arbitrumTxnBatchesMock from 'mocks/arbitrum/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ArbitrumL2TxnBatches from './ArbitrumL2TxnBatches';

test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  await mockTextAd();
  await mockApiResponse('general:arbitrum_l2_txn_batches', arbitrumTxnBatchesMock.baseResponse);
  await mockApiResponse('general:arbitrum_l2_txn_batches_count', 9927);

  const component = await render(<ArbitrumL2TxnBatches/>);
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
    test.slow();
    await mockEnvs(ENVS_MAP.arbitrumRollup);
    await mockTextAd();
    await mockApiResponse('general:arbitrum_l2_txn_batches', arbitrumTxnBatchesMock.baseResponse);
    await mockApiResponse('general:arbitrum_l2_txn_batches_count', 9927);

    const component = await render(<ArbitrumL2TxnBatches/>);
    await expect(component).toHaveScreenshot();
  });
});
