import { Box } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { apps as appsMock } from 'mocks/apps/apps';
import { test, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Marketplace from './Marketplace';

const ESSENTIAL_DAPPS_CONFIG = JSON.stringify({
  swap: { chains: [ config.chain.id ], fee: '0.004', integrator: 'blockscout' },
  revoke: { chains: [ config.chain.id ] },
  multisend: { chains: [ config.chain.id ] },
});

const MARKETPLACE_BANNER_CONTENT_URL = 'https://localhost/marketplace-banner.html';
const MARKETPLACE_BANNER_LINK_URL = 'https://example.com';

test.beforeEach(async({ mockEnvs, mockAssetResponse, mockApiResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_ENABLED', 'true' ],
  ]);
  await mockApiResponse('admin:marketplace_dapps', appsMock, { pathParams: { chainId: config.chain.id } });
  await Promise.all(appsMock.map(app => mockAssetResponse(app.logo, './playwright/mocks/image_s.jpg')));
});

test('base view +@dark-mode', async({ render, page }) => {
  await page.evaluate(() => window.localStorage.setItem('favoriteApps', '["hop-exchange"]'));

  const component = await render(<Marketplace/>);

  await expect(component).toHaveScreenshot();
});

test('with featured app +@dark-mode', async({ render, mockEnvs, page }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_FEATURED_APP', 'hop-exchange' ],
  ]);
  await page.evaluate(() => window.localStorage.setItem('favoriteApps', '["hop-exchange"]'));

  const component = await render(<Marketplace/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with banner +@dark-mode', async({ render, mockEnvs, mockConfigResponse, page }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL ],
    [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL', MARKETPLACE_BANNER_LINK_URL ],
  ]);
  await mockConfigResponse('MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL, './playwright/mocks/page.html', true);
  const component = await render(<Marketplace/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with essential dapps +@dark-mode', async({ render, mockEnvs, mockConfigResponse, page }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG', ESSENTIAL_DAPPS_CONFIG ],
    [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL ],
    [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL', MARKETPLACE_BANNER_LINK_URL ],
  ]);
  await mockConfigResponse('MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL, './playwright/mocks/page.html', true);
  const component = await render(<Marketplace/>);
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

// I had a memory error while running tests in GH actions
// separate run for mobile tests fixes it
test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, page }) => {
    await page.evaluate(() => window.localStorage.setItem('favoriteApps', '["hop-exchange"]'));
    const component = await render(
      <Box>
        { /* Added a fake header because without the ActionBar works incorrectly without it */ }
        <Box h="100px" backgroundColor="#dbdbdb" p={ 1 }>
          Header
        </Box>
        <Marketplace/>
      </Box>,
    );

    await expect(component).toHaveScreenshot();
  });

  test('with featured app', async({ render, mockEnvs, page }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_MARKETPLACE_FEATURED_APP', 'hop-exchange' ],
    ]);
    await page.evaluate(() => window.localStorage.setItem('favoriteApps', '["hop-exchange"]'));
    const component = await render(<Marketplace/>);

    await expect(component).toHaveScreenshot();
  });

  test('with banner', async({ render, mockEnvs, mockConfigResponse }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL ],
      [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL', MARKETPLACE_BANNER_LINK_URL ],
    ]);
    await mockConfigResponse('MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL, './playwright/mocks/page.html', true);
    const component = await render(<Marketplace/>);

    await expect(component).toHaveScreenshot();
  });

  test('with essential dapps', async({ render, mockEnvs, mockConfigResponse }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG', ESSENTIAL_DAPPS_CONFIG ],
      [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL ],
      [ 'NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL', MARKETPLACE_BANNER_LINK_URL ],
    ]);
    await mockConfigResponse('MARKETPLACE_BANNER_CONTENT_URL', MARKETPLACE_BANNER_CONTENT_URL, './playwright/mocks/page.html', true);
    const component = await render(<Marketplace/>);
    await expect(component).toHaveScreenshot();
  });
});
