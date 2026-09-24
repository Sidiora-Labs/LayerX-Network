import React from 'react';

import { txnBatchesData } from 'mocks/zkEvm/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ZkEvmL2TxnBatches from './ZkEvmL2TxnBatches';

test('base view +@mobile', async({ render, mockTextAd, mockEnvs, mockApiResponse }) => {
  test.slow();
  await mockEnvs(ENVS_MAP.zkEvmRollup);
  await mockTextAd();
  await mockApiResponse('general:zkevm_l2_txn_batches', txnBatchesData);
  await mockApiResponse('general:zkevm_l2_txn_batches_count', 9927);
  const component = await render(<ZkEvmL2TxnBatches/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
