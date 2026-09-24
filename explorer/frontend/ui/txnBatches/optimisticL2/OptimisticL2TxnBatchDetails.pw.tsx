import type { UseQueryResult } from '@tanstack/react-query';
import React from 'react';

import type { OptimismL2TxnBatch } from 'types/api/optimisticL2';

import type { ResourceError } from 'lib/api/resources';
import * as txnBatchesMock from 'mocks/optimism/txnBatches';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import OptimisticL2TxnBatchDetails from './OptimisticL2TxnBatchDetails';

const hooksConfig = {
  router: {
    query: { number: '1' },
  },
};

test.beforeEach(async({ mockEnvs }) => {
  await mockEnvs(ENVS_MAP.optimisticRollup);
});

test('call data blob container +@mobile', async({ render }) => {
  const query = {
    data: txnBatchesMock.txnBatchTypeCallData,
  } as UseQueryResult<OptimismL2TxnBatch, ResourceError>;
  const component = await render(<OptimisticL2TxnBatchDetails query={ query }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test('celestia blob container +@mobile', async({ render }) => {
  const query = {
    data: txnBatchesMock.txnBatchTypeCelestia,
  } as UseQueryResult<OptimismL2TxnBatch, ResourceError>;
  const component = await render(<OptimisticL2TxnBatchDetails query={ query }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test('EIP-4844 blob container +@mobile', async({ render }) => {
  const query = {
    data: txnBatchesMock.txnBatchTypeEip4844,
  } as UseQueryResult<OptimismL2TxnBatch, ResourceError>;
  const component = await render(<OptimisticL2TxnBatchDetails query={ query }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test('Eigenda blob container +@mobile', async({ render }) => {
  const query = {
    data: txnBatchesMock.txnBatchTypeEigenda,
  } as UseQueryResult<OptimismL2TxnBatch, ResourceError>;
  const component = await render(<OptimisticL2TxnBatchDetails query={ query }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
