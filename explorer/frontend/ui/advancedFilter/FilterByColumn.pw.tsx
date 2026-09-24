/* eslint-disable react/jsx-no-bind */
import React from 'react';

import { test, expect } from 'playwright/lib';
import type { ColumnsIds } from 'ui/advancedFilter/constants';

import FilterByColumn from './FilterByColumn';

const columns: Array<ColumnsIds> = [
  'type',
  'method',
  'age',
  'or_and',
  'from',
  'to',
  'amount',
  'asset',
];

const filters = {
  transaction_types: [ 'coin_transfer' as const ],
  methods: [ '0xa9059cbb' ],
  age: '7d' as const,
  address_relation: 'or' as const,
  from_address_hashes_to_include: [ '0x1230000000000000000000000000000000000000' ],
  to_address_hashes_to_include: [ '0x4560000000000000000000000000000000000000' ],
  amount_from: '100',
  token_contract_symbols_to_include: [ 'ETH' ],
  token_contract_address_hashes_to_include: [ 'native' ],
};

const searchParams = {
  methods: {
    '0xa9059cbb': 'transfer',
  },
  tokens: {},
};

for (const column of columns) {
  test(`${ column } filter +@dark-mode`, async({ page, render, mockApiResponse }) => {
    await mockApiResponse('general:tokens', {
      items: [],
      next_page_params: null,
    });
    await mockApiResponse('general:advanced_filter_methods', [], { queryParams: { q: '' } });
    await render(
      <FilterByColumn
        filters={ filters }
        searchParams={ searchParams }
        column={ column }
        columnName="Test"
        handleFilterChange={ () => {} }
      />,
    );

    const filterButton = page.locator('button');
    await filterButton.click();
    const popover = page.locator('.chakra-popover__content');
    await expect(popover).toBeVisible();
    await expect(popover).toHaveScreenshot();
  });
}
