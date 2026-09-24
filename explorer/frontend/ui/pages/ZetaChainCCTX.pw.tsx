import React from 'react';

import { zetaChainCCTX, zetaChainCCTXFailed } from 'mocks/zetaChain/zetaChainCCTX';
import { zetaChainCCTXConfig } from 'mocks/zetaChain/zetaChainCCTXConfig';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ZetaChainCCTX from './ZetaChainCCTX';

const CCTX_CONFIG_URL = 'http://localhost:3000/zeta-config.json';
const CCTX_HASH = '0x1c1e7410d7dfefe6173cc11efa47221e85587d3831c69108121198e0b2a86657';

const hooksConfig = {
  router: {
    query: { hash: CCTX_HASH },
  },
};

test.beforeEach(async({ mockEnvs, mockTextAd, mockConfigResponse, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.zetaChain);
  await mockTextAd();
  await mockConfigResponse('NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', CCTX_CONFIG_URL, zetaChainCCTXConfig);
  await mockAssetResponse(zetaChainCCTXConfig[1].chain_logo, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(zetaChainCCTXConfig[2].chain_logo, './playwright/mocks/image_svg.svg');
});

test('successful transaction +@dark-mode +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('zetachain:transaction', zetaChainCCTX, {
    queryParams: { cctx_id: CCTX_HASH },
  });

  const component = await render(<ZetaChainCCTX/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test('failed transaction +@dark-mode +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('zetachain:transaction', zetaChainCCTXFailed, {
    queryParams: { cctx_id: CCTX_HASH },
  });

  const component = await render(<ZetaChainCCTX/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});
