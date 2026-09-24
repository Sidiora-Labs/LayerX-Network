import React from 'react';

import config from 'configs/app';
import * as verifiedAddressesMocks from 'mocks/account/verifiedAddresses';
import { token as contract } from 'mocks/address/address';
import { tokenInfo, tokenCounters, bridgedTokenA } from 'mocks/tokens/tokenInfo';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Token from './Token';

const hash = tokenInfo.address_hash;
const chainId = config.chain.id;

const hooksConfig = {
  router: {
    query: { hash, tab: 'token_transfers' },
    isReady: true,
  },
};

// FIXME
// test cases which use socket cannot run in parallel since the socket server always run on the same port
test.describe.configure({ mode: 'serial' });

test.beforeEach(async({ mockApiResponse, mockTextAd }) => {
  await mockApiResponse('general:token', tokenInfo, { pathParams: { hash } });
  await mockApiResponse('general:address', contract, { pathParams: { hash } });
  await mockApiResponse('general:token_counters', tokenCounters, { pathParams: { hash } });
  await mockApiResponse('general:token_transfers', { items: [], next_page_params: null }, { pathParams: { hash } });
  await mockTextAd();
});

test('base view', async({ render, page, createSocket }) => {
  const component = await render(<Token/>, { hooksConfig }, { withSocket: true });

  const socket = await createSocket();
  await socketServer.joinChannel(socket, `tokens:${ hash }`);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with verified info', async({ render, page, createSocket, mockApiResponse, mockAssetResponse }) => {
  test.slow();
  await mockApiResponse('contractInfo:token_verified_info', verifiedAddressesMocks.TOKEN_INFO_APPLICATION.APPROVED, { pathParams: { chainId, hash } });
  await mockAssetResponse(tokenInfo.icon_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(<Token/>, { hooksConfig }, { withSocket: true });

  const socket = await createSocket();
  const channel = await socketServer.joinChannel(socket, `tokens:${ hash }`);
  socketServer.sendMessage(socket, channel, 'total_supply', { total_supply: 10 ** 20 });
  await component.getByText('100 ARIA').waitFor({ state: 'visible', timeout: 10_000 });

  await page.getByLabel('Show info').click();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('bridged token', async({ render, page, createSocket, mockApiResponse, mockAssetResponse, mockEnvs }) => {
  const hash = bridgedTokenA.address_hash;
  const hooksConfig = {
    router: {
      query: { hash, tab: 'token_transfers' },
    },
  };

  await mockEnvs(ENVS_MAP.bridgedTokens);
  await mockApiResponse('general:token', bridgedTokenA, { pathParams: { hash } });
  await mockApiResponse('general:address', contract, { pathParams: { hash } });
  await mockApiResponse('general:token_counters', tokenCounters, { pathParams: { hash } });
  await mockApiResponse('general:token_transfers', { items: [], next_page_params: null }, { pathParams: { hash } });
  await mockApiResponse('contractInfo:token_verified_info', verifiedAddressesMocks.TOKEN_INFO_APPLICATION.APPROVED, { pathParams: { chainId, hash } });
  await mockAssetResponse(tokenInfo.icon_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(<Token/>, { hooksConfig }, { withSocket: true });
  const socket = await createSocket();
  await socketServer.joinChannel(socket, `tokens:${ hash.toLowerCase() }`);
  await component.getByText('369,000,000 HyFi').waitFor({ state: 'visible' });

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('scam token', async({ render, page, createSocket, mockApiResponse, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_TOKEN_SCAM_TOGGLE_ENABLED', 'true' ],
  ]);
  await mockApiResponse('general:token', { ...tokenInfo, reputation: 'scam' }, { pathParams: { hash } });
  const component = await render(<Token/>, { hooksConfig }, { withSocket: true });

  const socket = await createSocket();
  await socketServer.joinChannel(socket, `tokens:${ hash }`);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, page, createSocket }) => {
    test.slow();
    const component = await render(<Token/>, { hooksConfig }, { withSocket: true });
    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `tokens:${ hash }`);
    socketServer.sendMessage(socket, channel, 'total_supply', { total_supply: 10 ** 20 });
    await component.getByText('100 ARIA').waitFor({ state: 'visible', timeout: 10_000 });

    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

  test('with verified info', async({ render, page, createSocket, mockApiResponse, mockAssetResponse }) => {
    test.slow();
    await mockApiResponse('contractInfo:token_verified_info', verifiedAddressesMocks.TOKEN_INFO_APPLICATION.APPROVED, { pathParams: { chainId, hash } });
    await mockAssetResponse(tokenInfo.icon_url as string, './playwright/mocks/image_s.jpg');

    const component = await render(<Token/>, { hooksConfig }, { withSocket: true });
    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `tokens:${ hash }`);
    socketServer.sendMessage(socket, channel, 'total_supply', { total_supply: 10 ** 20 });
    await component.getByText('100 ARIA').waitFor({ state: 'visible', timeout: 10_000 });

    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });
});
