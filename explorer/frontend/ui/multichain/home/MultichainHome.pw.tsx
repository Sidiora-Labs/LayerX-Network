import React from 'react';

import * as chainDataMock from 'mocks/multichain/chains';
import * as metricsMock from 'mocks/multichain/metrics';
import * as statsMock from 'mocks/multichain/stats';
import * as chainStatsMock from 'mocks/stats/index';
import * as txMock from 'mocks/txs/tx';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import MultichainHome from './MultichainHome';

test('base view +@mobile +@dark-mode', async({ mockApiResponse, render, page, mockMultichainConfig, mockEnvs, mockAssetResponse, injectMetaMaskProvider }) => {
  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
  await mockAssetResponse(chainStatsMock.base.coin_image as string, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/goose.png');
  await mockAssetResponse(chainDataMock.chainB.logo as string, './playwright/mocks/duck.png');
  await injectMetaMaskProvider();

  await mockApiResponse('multichainAggregator:chain_metrics', metricsMock.chainMetrics);
  await mockApiResponse('multichainStats:pages_main', statsMock.mainPageStats);
  await mockApiResponse(
    'general:homepage_txs',
    [ txMock.base, txMock.withProtocolTag ],
    { chainConfig: chainDataMock.chainA },
  );
  await page.route('https://eth.blockscout.com/api/v2/stats', (route) => route.fulfill({
    status: 200,
    json: chainStatsMock.base,
  }));

  const component = await render(<MultichainHome/>);
  await component.getByRole('link', { name: 'OP Devnet' }).hover();
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
