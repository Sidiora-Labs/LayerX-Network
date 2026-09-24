import React from 'react';

import * as depositsMock from 'mocks/arbitrum/deposits';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ArbitrumL2Deposits from './ArbitrumL2Deposits';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  test.slow();
  await mockTextAd();
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  await mockApiResponse('general:arbitrum_l2_messages', depositsMock.baseResponse, { pathParams: { direction: 'to-rollup' } });
  await mockApiResponse('general:arbitrum_l2_messages_count', 3971111, { pathParams: { direction: 'to-rollup' } });

  const component = await render(<ArbitrumL2Deposits/>);

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
