import React from 'react';

import type { AddressesMetadataSearchResult } from 'types/api/addresses';

import * as addressMocks from 'mocks/address/address';
import { test, expect } from 'playwright/lib';

import AccountsLabelSearch from './AccountsLabelSearch';

const addresses: AddressesMetadataSearchResult = {
  items: [
    {
      ...addressMocks.withName,
      transactions_count: '1',
      coin_balance: '12345678901234567890000',
    },
    {
      ...addressMocks.token,
      transactions_count: '109123890123',
      coin_balance: '22222345678901234567890000',
      ens_domain_name: null,
    },
    {
      ...addressMocks.withoutName,
      transactions_count: '11',
      coin_balance: '1000000000000000000',
    },
    {
      ...addressMocks.eoa,
      transactions_count: '420',
      coin_balance: null,
    },
  ],
  next_page_params: null,
};

const hooksConfig = {
  router: {
    query: {
      slug: 'euler-finance-exploit',
      tagType: 'generic',
      tagName: 'Euler finance exploit',
    },
  },
};

test('base view +@mobile', async({ render, mockTextAd, mockApiResponse }) => {
  await mockTextAd();
  await mockApiResponse(
    'general:addresses_metadata_search',
    addresses,
    {
      queryParams: {
        slug: 'euler-finance-exploit',
        tag_type: 'generic',
      },
    },
  );
  const component = await render(<AccountsLabelSearch/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
