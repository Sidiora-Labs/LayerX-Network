import React from 'react';

import type { GetAddressResponse } from '@blockscout/multichain-aggregator-types';

import * as chainDataMock from 'mocks/multichain/chains';
import * as searchMock from 'mocks/multichain/search';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import SearchResults from './SearchResults';

const SEARCH_TERM = 'usd';

const hooksConfig = {
  router: {
    query: { q: SEARCH_TERM },
  },
};

test.beforeEach(async({ mockApiResponse, mockMultichainConfig, mockEnvs, mockAssetResponse, mockTextAd }) => {

  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
  await mockTextAd();

  await mockApiResponse('multichainAggregator:search_addresses', {
    items: [
      searchMock.searchAddressesA,
      ...Array(10).fill('').map((_, index) => ({
        ...searchMock.searchAddressesB,
        hash: `0x00883b68A6EcF2ea3D47BD735E5125a0B7625B5${ index }`,
      })) as Array<GetAddressResponse>,
    ],
    next_page_params: undefined,
  }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_tokens', {
    items: [ searchMock.searchTokenA ],
    next_page_params: undefined,
  }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_blocks', { items: [ ], next_page_params: undefined }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_block_numbers', { items: [ ], next_page_params: undefined }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_transactions', { items: [ ], next_page_params: undefined }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_nfts', { items: [ ], next_page_params: undefined }, { queryParams: { q: SEARCH_TERM } });
  await mockApiResponse('multichainAggregator:search_domains', { items: [ ], next_page_params: undefined }, { queryParams: { q: SEARCH_TERM } });

  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/image_s.jpg');
});

test.describe('desktop', () => {
  test.use({ viewport: pwConfig.viewport.xl });

  test('base view', async({ render }) => {
    const component = await render(
      <SearchResults/>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();
  });
});

test.describe('mobile', () => {
  test.use({ viewport: pwConfig.viewport.mobile });

  test('base view', async({ render }) => {
    const component = await render(
      <SearchResults/>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();
  });
});
