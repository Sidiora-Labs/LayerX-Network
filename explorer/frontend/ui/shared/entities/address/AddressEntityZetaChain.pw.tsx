import React from 'react';

import * as addressMock from 'mocks/address/address';
import { zetaChainCCTXConfig } from 'mocks/zetaChain/zetaChainCCTXConfig';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import AddressEntityZetaChain from './AddressEntityZetaChain';

test.use({ viewport: { width: 180, height: 140 } });

const CCTX_CONFIG_URL = 'http://localhost:3000/zeta-config.json';

test.beforeEach(async({ mockEnvs, mockConfigResponse }) => {
  await mockEnvs([
    ...ENVS_MAP.zetaChain,
    [ 'NEXT_PUBLIC_NETWORK_ID', '7001' ],
    [ 'NEXT_PUBLIC_NETWORK_ICON', 'https://example.com/zeta.svg' ],
    [ 'NEXT_PUBLIC_NETWORK_ICON_DARK', 'https://example.com/zeta-dark.jpg' ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', CCTX_CONFIG_URL, zetaChainCCTXConfig);
});

test('with chain icon', async({ render, mockAssetResponse }) => {
  await mockAssetResponse('https://example.com/sepolia-logo.svg', './playwright/mocks/image_svg.svg');
  const component = await render(
    <AddressEntityZetaChain
      address={ addressMock.withoutName }
      chainId="11155111"
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('with chain icon stub +@dark-mode', async({ render }) => {
  const component = await render(
    <AddressEntityZetaChain
      address={ addressMock.withoutName }
      chainId="11155111"
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('with current chain icon +@dark-mode', async({ render, mockConfigResponse }) => {
  await mockConfigResponse('NEXT_PUBLIC_NETWORK_ICON', 'https://example.com/zeta.svg', './playwright/mocks/image_svg.svg', true);
  await mockConfigResponse('NEXT_PUBLIC_NETWORK_ICON_DARK', 'https://example.com/zeta-dark.jpg', './playwright/mocks/image_s.jpg', true);

  const component = await render(
    <AddressEntityZetaChain
      address={ addressMock.withoutName }
      chainId="7001"
    />,
  );

  await expect(component).toHaveScreenshot();
});
