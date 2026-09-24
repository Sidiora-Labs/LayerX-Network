import React from 'react';

import { FEATURED_NETWORKS } from 'mocks/config/network';
import { test, expect } from 'playwright/lib';

import NetworkMenu from './NetworkMenu';

test('tabs +@dark-mode', async({ render, mockEnvs, mockConfigResponse, mockAssetResponse, page }) => {
  const FEATURED_NETWORKS_URL = 'https://localhost:3000/featured-networks.json';

  await mockEnvs([
    [ 'NEXT_PUBLIC_FEATURED_NETWORKS', FEATURED_NETWORKS_URL ],
    [ 'NEXT_PUBLIC_FEATURED_NETWORKS_ALL_LINK', 'https://example.com' ],
    [ 'NEXT_PUBLIC_FEATURED_NETWORKS_MODE', 'tabs' ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_FEATURED_NETWORKS', FEATURED_NETWORKS_URL, FEATURED_NETWORKS);
  await mockAssetResponse('https://localhost:3000/my-logo.png', './playwright/mocks/image_s.jpg');

  const component = await render(<NetworkMenu/>);
  await component.getByLabel('Network menu').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 500 } });

  await page.getByRole('link', { name: 'Arbitrum' }).hover();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 500 } });
});

test('list +@dark-mode', async({ render, mockEnvs, mockConfigResponse, mockAssetResponse, page }) => {
  const FEATURED_NETWORKS_URL = 'https://localhost:3000/featured-networks.json';

  await mockEnvs([
    [ 'NEXT_PUBLIC_FEATURED_NETWORKS', FEATURED_NETWORKS_URL ],
    [ 'NEXT_PUBLIC_FEATURED_NETWORKS_ALL_LINK', 'https://example.com' ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_FEATURED_NETWORKS', FEATURED_NETWORKS_URL, FEATURED_NETWORKS);
  await mockAssetResponse('https://localhost:3000/my-logo.png', './playwright/mocks/image_s.jpg');

  const component = await render(<NetworkMenu/>);
  await component.getByLabel('Network menu').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 500 } });
});
