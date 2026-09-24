import React from 'react';

import * as depositsMock from 'mocks/arbitrum/withdrawals';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ArbitrumL2Withdrawals from './ArbitrumL2Withdrawals';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  test.slow();
  await mockTextAd();
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  await mockApiResponse('general:arbitrum_l2_messages', depositsMock.baseResponse, { pathParams: { direction: 'from-rollup' } });
  await mockApiResponse('general:arbitrum_l2_messages_count', 3971111, { pathParams: { direction: 'from-rollup' } });

  const component = await render(<ArbitrumL2Withdrawals/>);

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
