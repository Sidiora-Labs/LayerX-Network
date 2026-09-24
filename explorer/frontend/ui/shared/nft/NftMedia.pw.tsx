import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TokenInstance } from 'types/api/token';

import * as tokenInfoMock from 'mocks/tokens/tokenInfo';
import { test, expect } from 'playwright/lib';

import NftMedia from './NftMedia';

const TOKEN_ID = '123';
const TOKEN_HASH = tokenInfoMock.tokenInfoERC721a.address_hash;

test.describe('no url', () => {
  test.use({ viewport: { width: 250, height: 250 } });
  test('preview +@dark-mode', async({ render }) => {
    const data = {
      id: TOKEN_ID,
      image_url: null,
      animation_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;
    const component = await render(<NftMedia data={ data }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with fallback', async({ render, mockAssetResponse }) => {
    const IMAGE_URL = 'https://localhost:3000/my-image.jpg';
    const data = {
      id: TOKEN_ID,
      image_url: IMAGE_URL,
      animation_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;

    await mockAssetResponse(IMAGE_URL, './playwright/mocks/image_long.jpg');
    const component = await render(<NftMedia data={ data }/>);
    await expect(component).toHaveScreenshot();
  });

  test('non-media url and fallback', async({ render, page, mockAssetResponse }) => {
    const ANIMATION_URL = 'https://localhost:3000/my-animation.m3u8';
    const ANIMATION_MEDIA_TYPE_API_URL = `/node-api/tokens/${ TOKEN_HASH }/instances/${ TOKEN_ID }/media-type?field=animation_url`;
    const IMAGE_URL = 'https://localhost:3000/my-image.jpg';
    const data = {
      id: TOKEN_ID,
      animation_url: ANIMATION_URL,
      image_url: IMAGE_URL,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;

    await page.route(ANIMATION_MEDIA_TYPE_API_URL, (route) => {
      return route.fulfill({
        status: 200,
        json: { type: undefined },
      });
    });
    await mockAssetResponse(IMAGE_URL, './playwright/mocks/image_long.jpg');

    const component = await render(<NftMedia data={ data }/>);
    await expect(component).toHaveScreenshot();
  });
});

test.describe('image', () => {
  const MEDIA_URL = 'https://localhost:3000/my-image.jpg';

  test.beforeEach(async({ mockAssetResponse }) => {
    await mockAssetResponse(MEDIA_URL, './playwright/mocks/image_long.jpg');
  });

  test('preview +@dark-mode', async({ render, page }) => {
    const data = {
      id: TOKEN_ID,
      animation_url: MEDIA_URL,
      image_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;
    await render(
      <Box boxSize="250px">
        <NftMedia data={ data } size="md"/>
      </Box>,
    );
    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 250, height: 250 } });
  });

  test('preview with thumbnails', async({ render, page, mockAssetResponse }) => {
    const THUMBNAIL_URL = 'https://localhost:3000/my-image-250.jpg';
    const data = {
      id: TOKEN_ID,
      animation_url: MEDIA_URL,
      image_url: null,
      thumbnails: {
        '500x500': THUMBNAIL_URL,
      },
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;
    await mockAssetResponse(THUMBNAIL_URL, './playwright/mocks/image_md.jpg');
    await render(
      <Box boxSize="250px">
        <NftMedia data={ data } size="md"/>
      </Box>,
    );
    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 250, height: 250 } });
  });

  test('preview hover', async({ render, page }) => {
    const data = {
      id: TOKEN_ID,
      animation_url: MEDIA_URL,
      image_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;
    const component = await render(<NftMedia data={ data } w="250px" size="md"/>);
    await component.getByRole('img', { name: 'Token instance image' }).hover();
    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 250, height: 250 } });
  });

  test('fullscreen +@dark-mode +@mobile', async({ render, page }) => {
    const data = {
      id: TOKEN_ID,
      animation_url: MEDIA_URL,
      image_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;
    const component = await render(<NftMedia data={ data } withFullscreen w="250px"/>);
    await component.getByRole('img', { name: 'Token instance image' }).click();
    await expect(page).toHaveScreenshot();
  });
});

test.describe('page', () => {
  test.use({ viewport: { width: 250, height: 250 } });

  const MEDIA_URL = 'https://localhost:3000/page.html';
  const MEDIA_TYPE_API_URL = `/node-api/tokens/${ TOKEN_HASH }/instances/${ TOKEN_ID }/media-type?field=animation_url`;

  test.beforeEach(async({ page, mockAssetResponse }) => {
    await mockAssetResponse(MEDIA_URL, './playwright/mocks/page.html');
    await page.route(MEDIA_TYPE_API_URL, (route) => route.fulfill({
      status: 200,
      json: { type: 'html' },
    }));
  });

  test('preview +@dark-mode', async({ render }) => {
    const data = {
      id: TOKEN_ID,
      animation_url: MEDIA_URL,
      image_url: null,
      token: tokenInfoMock.tokenInfoERC721a,
    } as TokenInstance;

    const component = await render(<NftMedia data={ data }/>);
    await expect(component).toHaveScreenshot();
  });
});
