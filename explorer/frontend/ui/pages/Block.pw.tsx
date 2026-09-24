import React from 'react';
import { numberToHex } from 'viem';

import config from 'configs/app';
import * as blockMock from 'mocks/blocks/block';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import Block from './Block';

const height = String(blockMock.base.height);
const hooksConfig = {
  router: {
    query: { height_or_hash: height },
  },
};

test.beforeEach(async({ mockTextAd }) => {
  await mockTextAd();
});

test('degradation view, details tab', async({ render, mockApiResponse, mockRpcResponse, page }) => {
  test.slow();
  await mockApiResponse('general:block', null as never, { pathParams: { height_or_hash: height }, status: 500 });
  await mockRpcResponse([
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ 'latest', false ],
      ReturnType: {
        ...blockMock.rpcBlockBase,
        number: String(Number(height) + 1_000) as `0x${ string }`,
      },
    },
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ numberToHex(Number(height)), false ],
      ReturnType: blockMock.rpcBlockBase,
    },
  ]);

  const component = await render(<Block/>, { hooksConfig });
  await page.waitForResponse(config.chain.rpcUrls[0]);

  await expect(component).toHaveScreenshot();
});

test('degradation view, txs tab', async({ render, mockApiResponse, mockRpcResponse, page }) => {
  test.slow();
  const hooksConfig = {
    router: {
      query: { height_or_hash: height, tab: 'txs' },
    },
  };

  await mockApiResponse('general:block', blockMock.base, { pathParams: { height_or_hash: height } });
  await mockApiResponse('general:block_txs', null as never, { pathParams: { height_or_hash: height }, status: 500 });
  await mockRpcResponse([
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ 'latest', false ],
      ReturnType: {
        ...blockMock.rpcBlockWithTxsInfo,
        number: String(Number(height) + 1_000) as `0x${ string }`,
      },
    },
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ numberToHex(Number(height)), true ],
      ReturnType: blockMock.rpcBlockWithTxsInfo,
    },
  ]);

  const component = await render(<Block/>, { hooksConfig });
  await page.waitForResponse(config.chain.rpcUrls[0]);

  await expect(component).toHaveScreenshot();
});

test('degradation view, withdrawals tab', async({ render, mockApiResponse, mockRpcResponse, mockEnvs, page }) => {
  test.slow();
  const hooksConfig = {
    router: {
      query: { height_or_hash: height, tab: 'withdrawals' },
    },
  };

  await mockEnvs(ENVS_MAP.beaconChain);
  await mockApiResponse('general:block', blockMock.withWithdrawals, { pathParams: { height_or_hash: height } });
  await mockApiResponse('general:block_withdrawals', null as never, { pathParams: { height_or_hash: height }, status: 500 });
  await mockRpcResponse([
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ 'latest', false ],
      ReturnType: {
        ...blockMock.rpcBlockBase,
        number: String(Number(height) + 1_000) as `0x${ string }`,
      },
    },
    {
      Method: 'eth_getBlockByNumber',
      Parameters: [ numberToHex(Number(height)), false ],
      ReturnType: blockMock.rpcBlockBase,
    },
  ]);

  const component = await render(<Block/>, { hooksConfig });
  await page.waitForResponse(config.chain.rpcUrls[0]);

  await expect(component).toHaveScreenshot();
});
