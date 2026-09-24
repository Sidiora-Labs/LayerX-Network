import React from 'react';

import * as zetaChainCCTXType from '@blockscout/zetachain-cctx-types';

import { zetaChainCCTXList } from 'mocks/zetaChain/zetaChainCCTX';
import { zetaChainCCTXConfig } from 'mocks/zetaChain/zetaChainCCTXConfig';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import LatestZetaChainCCTXs from './LatestZetaChainCCTXs';

const CCTX_CONFIG_URL = 'http://localhost:3000/zeta-config.json';

test.describe.configure({ mode: 'serial' });

test.beforeEach(async({ mockEnvs, mockConfigResponse, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.zetaChain);
  await mockConfigResponse('NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', CCTX_CONFIG_URL, zetaChainCCTXConfig);
  await mockAssetResponse(zetaChainCCTXConfig[1].chain_logo, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(zetaChainCCTXConfig[2].chain_logo, './playwright/mocks/image_svg.svg');
});

test('base view +@dark-mode', async({ render, mockApiResponse }) => {
  await mockApiResponse('zetachain:transactions', zetaChainCCTXList, {
    queryParams: {
      limit: 8,
      offset: 0,
      direction: zetaChainCCTXType.Direction.DESC,
    },
  });

  const component = await render(<LatestZetaChainCCTXs/>);

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', async({ render, mockApiResponse }) => {
    await mockApiResponse('zetachain:transactions', zetaChainCCTXList, {
      queryParams: {
        limit: 3,
        offset: 0,
        direction: zetaChainCCTXType.Direction.DESC,
      },
    });

    const component = await render(<LatestZetaChainCCTXs/>);

    await expect(component).toHaveScreenshot();
  });
});
