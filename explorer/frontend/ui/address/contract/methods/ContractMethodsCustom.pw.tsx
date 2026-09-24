import type { BrowserContext } from '@playwright/test';
import React from 'react';
import type { Abi } from 'viem';

import * as addressMock from 'mocks/address/address';
import * as methodsMock from 'mocks/contract/methods';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { test, expect } from 'playwright/lib';

import ContractMethodsCustom from './ContractMethodsCustom';

const addressHash = addressMock.hash;

const authTest = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

authTest('without data', async({ render }) => {
  const hooksConfig = {
    router: {
      query: { hash: addressHash, tab: 'read_write_custom_methods' },
    },
  };

  const component = await render(<ContractMethodsCustom/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

authTest('with data', async({ render, mockApiResponse }) => {
  const abi: Abi = [ ...methodsMock.read, ...methodsMock.write ] as Abi;
  await mockApiResponse('general:custom_abi', [ {
    abi,
    contract_address_hash: addressHash,
    contract_address: addressMock.withName,
    id: 1,
    name: 'Test',
  } ]);
  const hooksConfig = {
    router: {
      query: { hash: addressHash, tab: 'read_write_custom_methods' },
    },
  };

  const component = await render(<ContractMethodsCustom/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
