import type { Page } from '@playwright/test';
import React from 'react';

import { apps as appsMock } from 'mocks/apps/apps';
import * as searchMock from 'mocks/search/index';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import SearchBarMobile from './SearchBarMobile';

// Helper function to open search drawer and wait for input
const openSearchDrawer = async(page: Page, isHeroBanner = false) => {
  if (isHeroBanner) {
    // For hero banner, click the search input trigger
    await page.getByLabel(/Search/i).first().click();
    // Wait for the search input to be visible (use last() for the opened drawer input)
    await page.getByPlaceholder(/search/i).last().waitFor({ state: 'visible' });
  } else {
    // For regular mode, click the search icon button
    await page.getByRole('button').click();
    // Wait for the drawer to open and search input to be visible
    await page.getByPlaceholder(/search/i).waitFor({ state: 'visible' });
  }
};

// Helper function to get the correct search input based on context
const getSearchInput = (page: Page, isHeroBanner = false) => {
  // In hero banner mode, use the last input (the opened drawer input)
  // In regular mode, there should only be one input after opening the drawer
  return isHeroBanner ? page.getByPlaceholder(/search/i).last() : page.getByPlaceholder(/search/i);
};

test.beforeEach(async({ mockAssetResponse, mockEnvs, mockTextAd }) => {
  await mockTextAd();
  await mockAssetResponse(searchMock.token1.icon_url as string, './playwright/mocks/image_s.jpg');
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_ENABLED', 'false' ],
  ]);
});

test.use({ viewport: devices['iPhone 13 Pro'].viewport });

test('search by token name +@dark-mode', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.token1,
    searchMock.token2,
  ], { queryParams: { q: 'o' } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('o');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by contract name', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.contract1,
    searchMock.contract2,
    searchMock.address2,
  ], { queryParams: { q: 'o' } });

  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('o');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by name homepage +@dark-mode', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
  ], { queryParams: { q: 'o' } });
  await render(<SearchBarMobile isHeroBanner/>);
  await openSearchDrawer(page, true);
  await getSearchInput(page, true).fill('o');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 700 } });
});

test('search by tag', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.label1,
  ], { queryParams: { q: 'o' } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('o');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by address hash', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.address1,
  ], { queryParams: { q: searchMock.address1.address_hash } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.address1.address_hash);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by meta tag', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.metatag1,
    searchMock.metatag2,
    searchMock.metatag3,
  ], { queryParams: { q: 'utko' } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('utko');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by block number', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.block1,
    searchMock.block2,
    searchMock.block3,
  ], { queryParams: { q: searchMock.block1.block_number } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(String(searchMock.block1.block_number));
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by block hash', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.block1,
  ], { queryParams: { q: searchMock.block1.block_hash } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.block1.block_hash);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by tx hash', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.tx1,
  ], { queryParams: { q: searchMock.tx1.transaction_hash } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.tx1.transaction_hash);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by tac operation hash', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.tacOperation1,
  ], { queryParams: { q: searchMock.tacOperation1.tac_operation.operation_id } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.tacOperation1.tac_operation.operation_id);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by blob hash', async({ render, page, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.dataAvailability);
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.blob1,
  ], { queryParams: { q: searchMock.blob1.blob_hash } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.blob1.blob_hash);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by domain name', async({ render, page, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.nameService);
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.domain1,
  ], { queryParams: { q: searchMock.domain1.ens_info.name } });

  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.domain1.ens_info.name);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search by user op hash', async({ render, page, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.userOps);
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.userOp1,
  ], { queryParams: { q: searchMock.tx1.transaction_hash } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill(searchMock.tx1.transaction_hash);
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('search with view all link', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
    ...Array(47).fill(searchMock.contract1),
  ], { queryParams: { q: 'o' } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('o');
  await page.waitForResponse(apiUrl);

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('scroll suggest to category', async({ render, page, mockApiResponse }) => {
  const apiUrl = await mockApiResponse('general:quick_search', [
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
    searchMock.token1,
    searchMock.token2,
    searchMock.contract1,
  ], { queryParams: { q: 'o' } });
  await render(<SearchBarMobile/>);
  await openSearchDrawer(page);
  await getSearchInput(page).fill('o');
  await page.waitForResponse(apiUrl);

  await page.getByRole('tab', { name: 'Addresses' }).click();

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
});

test('recent keywords suggest', async({ render, page }, { project }) => {
  await render(<SearchBarMobile/>);
  const recentKeywords = '["10x2d311959270e0bbdc1fc7bc6dbd8ad645c4dd8d6aa32f5f89d54629a924f112b",' +
    '"0x1d311959270e0bbdc1fc7bc6dbd8ad645c4dd8d6aa32f5f89d54629a924f112b","usd","bob"]';
  await page.evaluate((keywords) => window.localStorage.setItem('recent_search_keywords', keywords), recentKeywords);
  await openSearchDrawer(page);
  await page.getByText(project.name === 'mobile' ? '0x1d311959270e0bbdc1fc7bc6dbd...112b' : '0x1d311959270e0bbdc1fc7bc6dbd').isVisible();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 500 } });
});

test.describe('with apps', () => {
  const MARKETPLACE_CONFIG_URL = 'http://localhost:4000/marketplace-config.json';

  test('default view', async({ render, page, mockApiResponse, mockConfigResponse, mockAssetResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_MARKETPLACE_ENABLED', 'true' ],
      [ 'NEXT_PUBLIC_MARKETPLACE_CONFIG_URL', MARKETPLACE_CONFIG_URL ],
    ]);
    const apiUrl = await mockApiResponse('general:quick_search', [
      searchMock.token1,
    ], { queryParams: { q: 'o' } });
    await mockConfigResponse('NEXT_PUBLIC_MARKETPLACE_CONFIG_URL', MARKETPLACE_CONFIG_URL, appsMock);
    await mockAssetResponse(appsMock[0].logo, './playwright/mocks/image_s.jpg');
    await mockAssetResponse(appsMock[1].logo, './playwright/mocks/image_s.jpg');
    const hooksConfig = {
      router: {
        query: { q: 'o' },
      },
    };
    await render(<SearchBarMobile/>, { hooksConfig });
    await openSearchDrawer(page);
    await getSearchInput(page).fill('o');
    await page.waitForResponse(apiUrl);

    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
  });
});

test.describe('block countdown', () => {
  test('no results', async({ render, page, mockApiResponse }) => {
    const apiUrl = await mockApiResponse('general:quick_search', [], { queryParams: { q: '1234567890' } });
    await render(<SearchBarMobile/>);
    await openSearchDrawer(page);
    await getSearchInput(page).fill('1234567890');
    await page.waitForResponse(apiUrl);

    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
  });

  test('with results', async({ render, page, mockApiResponse }) => {
    const apiUrl = await mockApiResponse('general:quick_search', [
      { ...searchMock.token1, name: '1234567890123456789' },
    ], { queryParams: { q: '1234567890' } });
    await render(<SearchBarMobile/>);
    await openSearchDrawer(page);
    await getSearchInput(page).fill('1234567890');
    await page.waitForResponse(apiUrl);

    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 1200, height: 600 } });
  });
});
