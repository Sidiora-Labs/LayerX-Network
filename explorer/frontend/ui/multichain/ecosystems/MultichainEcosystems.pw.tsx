import React from 'react';

import * as chainDataMock from 'mocks/multichain/chains';
import * as statsMock from 'mocks/multichain/stats';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import MultichainEcosystems from './MultichainEcosystems';

test('base view +@mobile', async({ render, mockTextAd, mockApiResponse, mockEnvs, mockMultichainConfig, mockAssetResponse }) => {
  test.slow();
  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
  await mockTextAd();
  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/duck.png');
  await mockAssetResponse(chainDataMock.chainB.logo as string, './playwright/mocks/goose.png');
  await mockApiResponse('multichainAggregator:chain_metrics', statsMock.chainMetrics);

  const component = await render(<MultichainEcosystems/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
