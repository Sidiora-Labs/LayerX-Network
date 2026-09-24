import React from 'react';

import config from 'configs/app';
import * as addressMock from 'mocks/address/address';
import * as tokenMock from 'mocks/tokens/tokenInfo';
import * as tokenInstanceMock from 'mocks/tokens/tokenInstance';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import TokenInstance from './TokenInstance';

const hash = tokenMock.tokenInfo.address_hash;
const id = '42';

test.describe.configure({ mode: 'serial' });

test.beforeEach(async({ mockApiResponse, mockAssetResponse, mockTextAd }) => {
  await mockApiResponse('general:token', tokenMock.tokenInfo, { pathParams: { hash } });
  await mockApiResponse('general:address', addressMock.token, { pathParams: { hash } });
  await mockApiResponse('general:token_instance', tokenInstanceMock.unique, { pathParams: { hash, id } });
  await mockApiResponse('general:token_instance_transfers', { items: [], next_page_params: null }, { pathParams: { hash, id } });
  await mockApiResponse('general:token_instance_transfers_count', { transfers_count: 420 }, { pathParams: { hash, id } });
  await mockTextAd();
  for (const marketplace of config.UI.views.nft.marketplaces) {
    await mockAssetResponse(marketplace.logo_url, './playwright/mocks/image_svg.svg');
  }
  await mockAssetResponse(tokenInstanceMock.base.image_url as string, './playwright/mocks/image_md.jpg');
});

test('metadata update', async({ render, page, createSocket, mockApiResponse, mockAssetResponse }) => {
  const hooksConfig = {
    router: {
      query: { hash, id, tab: 'metadata' },
      pathname: '/token/[hash]/instance/[id]',
    },
  };
  const newMetadata = {
    attributes: [
      { value: 'yellow', trait_type: 'Color' },
      { value: 'Mrs. Duckie', trait_type: 'Name' },
    ],
    external_url: 'https://yellow-duck.nft',
    image_url: 'https://yellow-duck.nft/duck.jpg',
    animation_url: null,
    status: 'FRESH!!!',
    name: 'Carmelo Anthony',
    description: 'Updated description',
  };
  await mockApiResponse('general:token_instance_refresh_metadata', {} as never, { pathParams: { hash, id } });
  await mockAssetResponse(newMetadata.image_url, './playwright/mocks/image_long.jpg');

  const component = await render(<TokenInstance/>, { hooksConfig }, { withSocket: true });
  const socket = await createSocket();

  // take a screenshot of initial state
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });

  // open the menu, click the button and submit form
  await page.getByLabel('Address menu').click();
  await page.getByRole('menuitem', { name: 'Refresh metadata' }).click();

  // join socket channel
  const channel = await socketServer.joinChannel(socket, `token_instances:${ hash.toLowerCase() }`);

  // check that button is disabled
  await page.getByLabel('Address menu').click();
  await expect(page.getByRole('menuitem', { name: 'Refresh metadata' })).toBeDisabled();
  await page.getByLabel('Address menu').click();

  // take a screenshot of loading state
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });

  // send message in socket but with wrong token id
  socketServer.sendMessage(socket, channel, 'fetched_token_instance_metadata', {
    token_id: Number(id) + 1,
    fetched_metadata: newMetadata,
  });
  await expect(page.getByText(newMetadata.description)).toBeHidden();

  // send message in socket with correct token id
  socketServer.sendMessage(socket, channel, 'fetched_token_instance_metadata', {
    token_id: Number(id),
    fetched_metadata: newMetadata,
  });

  // take a screenshot of updated state
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('metadata update failed', async({ render, page }) => {
  const hooksConfig = {
    router: {
      query: { hash, id, tab: 'metadata' },
      pathname: '/token/[hash]/instance/[id]',
    },
  };

  const component = await render(<TokenInstance/>, { hooksConfig }, { withSocket: true });

  // open the menu, click the button and submit form
  await page.getByLabel('Address menu').click();
  await page.getByRole('menuitem', { name: 'Refresh metadata' }).click();

  // check that button is not disabled
  await page.getByLabel('Address menu').click();
  await expect(page.getByRole('menuitem', { name: 'Refresh metadata' })).toBeEnabled();
  await page.getByLabel('Address menu').click();

  // take a screenshot of error state
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
