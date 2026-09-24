import React from 'react';

import type { PaginationParams } from './types';

import { test, expect } from 'playwright/lib';

import Pagination from './Pagination';

test.use({ viewport: { width: 250, height: 50 } });

test('default view', async({ render }) => {
  const props: PaginationParams = {
    page: 2,
    isVisible: true,
    isLoading: false,
    hasNextPage: true,
    hasPages: true,
    canGoBackwards: false,
    onNextPageClick: () => {},
    onPrevPageClick: () => {},
    resetPage: () => {},
  };
  const component = await render(<Pagination { ...props } w="fit-content"/>);
  await expect(component).toHaveScreenshot();
});
