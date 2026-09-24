import { Box } from '@chakra-ui/react';
import React from 'react';

import { zetaChainCCTXList } from 'mocks/zetaChain/zetaChainCCTX';
import { zetaChainCCTXConfig } from 'mocks/zetaChain/zetaChainCCTXConfig';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import ZetaChainCCTXsTab from './ZetaChainCCTXsTab';

const CCTX_CONFIG_URL = 'http://localhost:3000/zeta-config.json';

test.beforeEach(async({ mockEnvs, mockConfigResponse, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.zetaChain);
  await mockConfigResponse('NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', CCTX_CONFIG_URL, zetaChainCCTXConfig);
  await mockAssetResponse(zetaChainCCTXConfig[1].chain_logo, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(zetaChainCCTXConfig[2].chain_logo, './playwright/mocks/image_svg.svg');
});

test('base view +@dark-mode', async({ render, mockApiResponse }) => {
  await mockApiResponse('zetachain:transactions', zetaChainCCTXList, {
    queryParams: {
      status_reduced: [ 'Success', 'Failed' ],
      limit: 50,
      offset: 0,
      direction: 'DESC',
    },
  });

  const component = await render(
    <Box pt={ 6 }>
      <ZetaChainCCTXsTab/>
    </Box>, {
      hooksConfig: {
        router: {
          query: { tab: 'cctx' },
        },
      },
    });

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view +@dark-mode', async({ render, mockApiResponse }) => {
    await mockApiResponse('zetachain:transactions', zetaChainCCTXList, {
      queryParams: {
        status_reduced: [ 'Success', 'Failed' ],
        limit: 50,
        offset: 0,
        direction: 'DESC',
      },
    });

    const component = await render(
      <Box pt="134px">
        <ZetaChainCCTXsTab/>
      </Box>, {
        hooksConfig: {
          router: {
            query: { tab: 'cctx' },
          },
        },
      });

    await expect(component).toHaveScreenshot();
  });
});
