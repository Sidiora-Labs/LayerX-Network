import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as tokenMock from 'mocks/tokens/tokenInfo';
import { test, expect } from 'playwright/lib';

import CsvExport from './CsvExport';

test('base view +@mobile +@dark-mode', async({ render, mockApiResponse }) => {
  const hooksConfig = {
    router: {
      query: { address: addressMock.hash, type: 'transactions', filterType: 'address', filterValue: 'from' },
    },
  };
  await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: addressMock.hash } });
  await mockApiResponse('general:config_csv_export', { limit: 42123 });

  const component = await render(<CsvExport/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test('token holders', async({ render, mockApiResponse }) => {
  const hooksConfig = {
    router: {
      query: { address: addressMock.hash, type: 'holders' },
    },
  };
  await mockApiResponse('general:address', addressMock.token, { pathParams: { hash: addressMock.hash } });
  await mockApiResponse('general:token', tokenMock.tokenInfo, { pathParams: { hash: addressMock.hash } });

  const component = await render(<CsvExport/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});
