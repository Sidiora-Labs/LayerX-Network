import React from 'react';
import type { Abi, AbiFunction } from 'viem';

import * as addressMock from 'mocks/address/address';
import * as methodsMock from 'mocks/contract/methods';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import ContractMethodsRegular from './ContractMethodsRegular';

const addressHash = addressMock.hash;

test('can read method', async({ render, mockContractReadResponse }) => {
  const hooksConfig = {
    router: {
      query: { hash: addressHash, tab: 'read_contract' },
    },
  };

  await mockContractReadResponse({
    abiItem: methodsMock.read[1],
    address: addressHash,
    result: [ 'USDC' ],
  });

  const component = await render(<ContractMethodsRegular abi={ methodsMock.read }/>, { hooksConfig });
  await component.getByText(/expand all/i).click();

  await expect(component.getByText('USDC')).toBeVisible({ timeout: 20_000 });
});

test('can simulate method', async({ render, mockContractReadResponse }) => {
  const ADDRESS = '0x0000000000000068F116a894984e2DB1123eB395';
  const AMOUNT = '123';
  const abiItem = methodsMock.write[1] as AbiFunction;

  const hooksConfig = {
    router: {
      query: { hash: addressHash, tab: 'write_contract' },
    },
  };

  await mockContractReadResponse({
    abiItem,
    address: addressHash,
    args: [ ADDRESS, AMOUNT ],
    rpcMethod: 'eth_estimateGas',
    result: 42000,
    noResultEncoding: true,
  });
  await mockContractReadResponse({
    abiItem,
    address: addressHash,
    args: [ ADDRESS, AMOUNT ],
    result: true,
  });

  const component = await render(<ContractMethodsRegular abi={ [ abiItem ] }/>, { hooksConfig });
  await component.getByPlaceholder('address').last().fill(ADDRESS);
  await component.getByPlaceholder('uint256').last().fill(AMOUNT);
  await component.getByText(/simulate/i).click();

  await expect(component).toHaveScreenshot();
});

test('all methods +@dark-mode', async({ render }) => {
  const hooksConfig = {
    router: {
      query: { hash: addressHash, tab: 'read_write_contract' },
    },
  };

  const abi: Abi = [ ...methodsMock.read, ...methodsMock.write ] as Abi;
  const component = await render(<ContractMethodsRegular abi={ abi }/>, { hooksConfig });
  await component.getByText(/expand all/i).click();
  await expect(component.getByText('HTTP request failed')).toBeVisible();

  await expect(component).toHaveScreenshot();
});

test.describe('all methods', () => {
  test.use({ viewport: pwConfig.viewport.mobile });

  test('mobile', async({ render }) => {
    test.slow();

    const hooksConfig = {
      router: {
        query: { hash: addressHash, tab: 'read_write_contract' },
      },
    };

    const abi: Abi = [ ...methodsMock.read, ...methodsMock.write ] as Abi;
    const component = await render(<ContractMethodsRegular abi={ abi }/>, { hooksConfig });
    await component.getByText(/expand all/i).click();
    // await expect(component.getByText('HTTP request failed')).toBeVisible();

    await expect(component).toHaveScreenshot();
  });
});
