import React from 'react';

import * as withdrawalsMock from 'mocks/zkEvm/withdrawals';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ZkEvmL2Withdrawals from './ZkEvmL2Withdrawals';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockTextAd();
  await mockEnvs(ENVS_MAP.zkEvmRollup);
  await mockApiResponse('general:zkevm_l2_withdrawals', withdrawalsMock.baseResponse);
  await mockApiResponse('general:zkevm_l2_withdrawals_count', 3971111);

  const component = await render(<ZkEvmL2Withdrawals/>);

  await expect(component).toHaveScreenshot();
});
