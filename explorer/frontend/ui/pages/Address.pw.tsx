import React from 'react';
import { numberToHex } from 'viem';

import config from 'configs/app';
import * as addressMock from 'mocks/address/address';
import * as addressCountersMock from 'mocks/address/counters';
import * as addressTabCountersMock from 'mocks/address/tabCounters';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Address from './Address';

const hooksConfig = {
  router: {
    query: { hash: addressMock.hash },
  },
};

test.beforeEach(async({ mockTextAd }) => {
  await mockTextAd();
});

test.describe('fetched bytecode', () => {
  test('should refetch address query', async({ render, mockApiResponse, createSocket, page }) => {
    const addressApiUrl = await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: addressMock.hash } });
    await mockApiResponse('general:address_counters', addressCountersMock.forValidator, { pathParams: { hash: addressMock.hash } });
    await mockApiResponse('general:address_tabs_counters', addressTabCountersMock.base, { pathParams: { hash: addressMock.hash } });
    await mockApiResponse('general:address_txs', { items: [], next_page_params: null }, { pathParams: { hash: addressMock.hash } });
    await render(<Address/>, { hooksConfig }, { withSocket: true });

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ addressMock.hash.toLowerCase() }`);
    socketServer.sendMessage(socket, channel, 'fetched_bytecode', { fetched_bytecode: '0x0123' });

    const request = await page.waitForRequest(addressApiUrl);

    expect(request).toBeTruthy();
  });
});

test('degradation view', async({ render, page, mockRpcResponse, mockApiResponse }) => {
  await mockApiResponse('general:address', null as never, { pathParams: { hash: addressMock.hash }, status: 500 });
  await mockApiResponse('general:address_counters', addressCountersMock.forValidator, { pathParams: { hash: addressMock.hash } });
  await mockApiResponse('general:address_tabs_counters', null as never, { pathParams: { hash: addressMock.hash }, status: 500 });
  await mockApiResponse('general:address_txs', null as never, { pathParams: { hash: addressMock.hash }, status: 500 });
  await mockRpcResponse([ {
    Method: 'eth_getBalance',
    Parameters: [ addressMock.hash, 'latest' ],
    ReturnType: numberToHex(1234567890123456),
  } ]);

  const component = await render(<Address/>, { hooksConfig });
  await page.waitForResponse(config.chain.rpcUrls[0]);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
