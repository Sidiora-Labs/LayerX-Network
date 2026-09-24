import { Box } from '@chakra-ui/react';
import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as tokensMock from 'mocks/address/tokens';
import { test, expect } from 'playwright/lib';

import AddressNetWorth from './AddressNetWorth';

const ADDRESS_HASH = addressMock.hash;
const ICON_URL = 'https://localhost:3000/my-icon.png';
const ICON_URL_PROMO = 'https://localhost:3000/my-icon-promo.png';

test.beforeEach(async({ mockApiResponse }) => {
  await mockApiResponse('general:address_tokens', tokensMock.erc20List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' } });
  await mockApiResponse('general:address_tokens', tokensMock.erc721List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' } });
  await mockApiResponse('general:address_tokens', tokensMock.erc1155List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' } });
  await mockApiResponse('general:address_tokens', tokensMock.erc404List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' } });
});

test('base view', async({ render }) => {
  const component = await render(<AddressNetWorth addressData={ addressMock.eoa } addressHash={ ADDRESS_HASH }/>);

  await expect(component).toHaveScreenshot();
});

test('with single multichain button internal +@dark-mode', async({ render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs([
    [
      'NEXT_PUBLIC_MULTICHAIN_BALANCE_PROVIDER_CONFIG',
      `[{"name": "duck", "dapp_id": "duck", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL }"}]` ],
  ]);
  await mockAssetResponse(ICON_URL, './playwright/mocks/image_svg.svg');

  const component = await render(<AddressNetWorth addressData={ addressMock.eoa } addressHash={ ADDRESS_HASH }/>);

  await expect(component).toHaveScreenshot();
});

test('with single multichain button external', async({ render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MULTICHAIN_BALANCE_PROVIDER_CONFIG', `[{"name": "duck", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL }"}]` ],
  ]);
  await mockAssetResponse(ICON_URL, './playwright/mocks/image_svg.svg');

  const component = await render(<AddressNetWorth addressData={ addressMock.eoa } addressHash={ ADDRESS_HASH }/>);

  await expect(component).toHaveScreenshot();
});

test('with two multichain button and promo', async({ render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MULTICHAIN_BALANCE_PROVIDER_CONFIG', `[
      {"name": "duck", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL }"},
      {"name": "duck2", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL }"},
      {"name": "duck3", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL_PROMO }", "promo": true}
    ]` ],
  ]);
  await mockAssetResponse(ICON_URL, './playwright/mocks/image_svg.svg');
  await mockAssetResponse(ICON_URL_PROMO, './playwright/mocks/image_s.jpg');

  const component = await render(<AddressNetWorth addressData={ addressMock.eoa } addressHash={ ADDRESS_HASH }/>);

  await expect(component).toHaveScreenshot();
});

test('with multichain button internal small screen', async({ render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs([
    [
      'NEXT_PUBLIC_MULTICHAIN_BALANCE_PROVIDER_CONFIG',
      `[{"name": "duck", "dapp_id": "duck", "url_template": "https://duck.url/{address}", "logo": "${ ICON_URL }"}]` ],
  ]);
  await mockAssetResponse(ICON_URL, './playwright/mocks/image_svg.svg');

  const component = await render(
    <Box w="300px"><AddressNetWorth addressData={ addressMock.eoa } addressHash={ ADDRESS_HASH }/></Box>,
  );

  await expect(component).toHaveScreenshot();
});
