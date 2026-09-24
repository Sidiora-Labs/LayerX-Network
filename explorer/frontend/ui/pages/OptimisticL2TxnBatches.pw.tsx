import React from 'react';

import { txnBatchesData } from 'mocks/optimism/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2TxnBatches from './OptimisticL2TxnBatches';

test('base view +@mobile', async({ render, mockTextAd, mockEnvs, mockApiResponse }) => {
  // test on mobile is flaky
  // my assumption is there is not enough time to calculate hashes truncation so component is unstable
  // so I raised the test timeout to check if it helps
  test.slow();
  await mockTextAd();
  await mockEnvs(ENVS_MAP.optimisticRollup);
  await mockApiResponse('general:optimistic_l2_txn_batches', txnBatchesData);
  await mockApiResponse('general:optimistic_l2_txn_batches_count', 1235016);
  const component = await render(<OptimisticL2TxnBatches/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
