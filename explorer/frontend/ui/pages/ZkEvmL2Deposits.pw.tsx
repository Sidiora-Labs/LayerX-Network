import React from 'react';

import * as depositsMock from 'mocks/zkEvm/deposits';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ZkEvmL2Deposits from './ZkEvmL2Deposits';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  test.slow();
  await mockTextAd();
  await mockEnvs(ENVS_MAP.zkEvmRollup);
  await mockApiResponse('general:zkevm_l2_deposits', depositsMock.baseResponse);
  await mockApiResponse('general:zkevm_l2_deposits_count', 3971111);

  const component = await render(<ZkEvmL2Deposits/>);

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
