import React from 'react';

import type { HighlightsBannerConfig } from 'types/homepage';

import { test, expect } from 'playwright/lib';

import Highlights from './Highlights';

const IMAGE_URL_1 = 'https://localhost:3000/my-image.png';
const IMAGE_URL_2 = 'https://localhost:3000/my-image-2.png';
const IMAGE_URL_3 = 'https://localhost:3000/my-image-3.png';
const HIGHLIGHTS_CONFIG_URL = 'https://localhost:3000/homepage-highlights-config.json';
const HIGHLIGHTS_CONFIG: Array<HighlightsBannerConfig> = [
  // no adaptive
  {
    title: 'Duck Deep into Transactions',
    description: 'Explore and track all blockchain transactions',
    side_img_url: [ IMAGE_URL_1 ],
    title_color: [ '#D9FE41' ],
    description_color: [ '#CBD0D7' ],
    background: [ '#06331B' ],
    redirect_url: 'https://example.com',
    is_pinned: true,
  },
  // adaptive
  {
    title: 'Geese Token Swap',
    description: 'Swap tokens across different protocols',
    title_color: [ '#1e40af', '#93c5fd' ],
    description_color: [ '#64748b', '#94a3b8' ],
    background: [
      'linear-gradient(82.75deg, #FDDCEF 0.08%, #FAF5FB 51.54%, #FFFBDB 104.15%)',
      'linear-gradient(82.75deg, #4F2D6A 0.08%, #3D425A 51.54%, #3A6024 104.15%)',
    ],
    side_img_url: [ IMAGE_URL_2, IMAGE_URL_3 ],
    page_path: '/essential-dapps/swap',
    is_pinned: true,
  },
  // default
  {
    title: 'Duckling Smart Contracts',
    description: 'Discover newly deployed smart contracts',
    is_pinned: true,
  },
];

test('three banners +@dark-mode', async({ render, mockEnvs, mockConfigResponse, mockAssetResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG', HIGHLIGHTS_CONFIG_URL ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG', HIGHLIGHTS_CONFIG_URL, HIGHLIGHTS_CONFIG);
  await mockAssetResponse(IMAGE_URL_1, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(IMAGE_URL_2, './playwright/mocks/image_md.jpg');
  await mockAssetResponse(IMAGE_URL_3, './playwright/mocks/image_long.jpg');

  const component = await render(<Highlights/>);

  await expect(component).toHaveScreenshot();
});

test('two banners', async({ render, mockEnvs, mockConfigResponse, mockAssetResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG', HIGHLIGHTS_CONFIG_URL ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG', HIGHLIGHTS_CONFIG_URL, HIGHLIGHTS_CONFIG.slice(0, 2));
  await mockAssetResponse(IMAGE_URL_1, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(IMAGE_URL_2, './playwright/mocks/image_md.jpg');
  await mockAssetResponse(IMAGE_URL_3, './playwright/mocks/image_long.jpg');

  const component = await render(<Highlights/>);

  await expect(component).toHaveScreenshot();
});
