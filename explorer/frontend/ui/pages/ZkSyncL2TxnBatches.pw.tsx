import React from 'react';

import * as zkSyncTxnBatchesMock from 'mocks/zkSync/zkSyncTxnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ZkSyncL2TxnBatches from './ZkSyncL2TxnBatches';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.zkSyncRollup);
  await mockTextAd();
  await mockApiResponse('general:zksync_l2_txn_batches', zkSyncTxnBatchesMock.baseResponse);
  await mockApiResponse('general:zksync_l2_txn_batches_count', 9927);

  const component = await render(<ZkSyncL2TxnBatches/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
