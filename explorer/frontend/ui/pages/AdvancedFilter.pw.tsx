import React from 'react';

import * as advancedFilterMock from 'mocks/advancedFilter/advancedFilter';
import { test, expect } from 'playwright/lib';

import AdvancedFilter from './AdvancedFilter';

test('base view +@dark-mode', async({ render, mockApiResponse, mockTextAd }) => {
  await mockTextAd();
  await mockApiResponse('general:advanced_filter', advancedFilterMock.baseResponse);
  await mockApiResponse('general:tokens', { items: [], next_page_params: null }, { queryParams: { limit: '7', q: '' } });
  await mockApiResponse('general:advanced_filter_methods', [], { queryParams: { q: '' } });

  const component = await render(<AdvancedFilter/>);

  await expect(component).toHaveScreenshot();
});
