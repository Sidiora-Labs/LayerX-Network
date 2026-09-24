import { Flex } from '@chakra-ui/react';
import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as tokensMock from 'mocks/address/tokens';
import { tokenInfoERC20c, tokenInfoERC20a } from 'mocks/tokens/tokenInfo';
import { test, expect, devices } from 'playwright/lib';
import MockAddressPage from 'ui/address/testUtils/MockAddressPage';

import TokenSelect from './TokenSelect';

const ASSET_URL = tokenInfoERC20a.icon_url as string;
const ADDRESS_HASH = addressMock.hash;
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};
const CLIPPING_AREA = { x: 0, y: 0, width: 360, height: 500 };

test.beforeEach(async({ mockApiResponse, mockAssetResponse }) => {
  await mockAssetResponse(ASSET_URL, './playwright/mocks/image_s.jpg');
  await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: ADDRESS_HASH }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc20List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc721List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc1155List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc404List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' }, times: 1 });
});

test('base view +@dark-mode', async({ render, page }) => {
  await render(
    <MockAddressPage>
      <Flex>
        <TokenSelect/>
      </Flex>
    </MockAddressPage>,
    { hooksConfig },
  );

  await page.getByRole('button', { name: /select/i }).click();

  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });

  await page.mouse.move(100, 200);
  await page.mouse.wheel(0, 1000);
  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, page }) => {
    await render(
      <MockAddressPage>
        <Flex>
          <TokenSelect/>
        </Flex>
      </MockAddressPage>,
      { hooksConfig },
    );

    await page.getByRole('button', { name: /select/i }).click();

    await expect(page).toHaveScreenshot();
  });
});

test('sort', async({ render, page }) => {
  await render(
    <MockAddressPage>
      <Flex>
        <TokenSelect/>
      </Flex>
    </MockAddressPage>,
    { hooksConfig },
  );
  await page.getByRole('button', { name: /select/i }).click();
  await page.locator('[aria-label="Sort ERC-20 tokens"]').click();

  await page.mouse.wheel(0, -1000);
  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });

  await page.mouse.move(100, 200);
  await page.mouse.wheel(0, 1000);
  await page.locator('[aria-label="Sort ERC-1155 tokens"]').click();

  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });
});

test('filter', async({ render, page }) => {
  await render(
    <MockAddressPage>
      <Flex>
        <TokenSelect/>
      </Flex>
    </MockAddressPage>,
    { hooksConfig },
  );
  await page.getByRole('button', { name: /select/i }).click();
  await page.getByPlaceholder('Search by token name').type('c');

  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });
});

test('long values', async({ render, page, mockApiResponse }) => {
  await mockApiResponse('general:address_tokens', {
    items: [ tokensMock.erc20LongSymbol, tokensMock.erc20BigAmount ], next_page_params: null,
  }, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' }, times: 1 });
  await mockApiResponse('general:address_tokens', {
    items: [ tokensMock.erc721LongSymbol ], next_page_params: null,
  }, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' }, times: 1 });
  await mockApiResponse('general:address_tokens', {
    items: [ tokensMock.erc1155LongId ], next_page_params: null,
  }, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc404List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' }, times: 1 });

  await render(
    <MockAddressPage>
      <Flex>
        <TokenSelect/>
      </Flex>
    </MockAddressPage>,
    { hooksConfig },
  );
  await page.getByRole('button', { name: /select/i }).click();

  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });
});

test('native token', async({ render, mockEnvs, page }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_ADDRESS_NATIVE_TOKEN_ADDRESS', tokenInfoERC20c.address_hash ],
  ]);

  await render(
    <MockAddressPage>
      <Flex>
        <TokenSelect/>
      </Flex>
    </MockAddressPage>,
    { hooksConfig },
  );
  await page.getByRole('button', { name: /select/i }).click();

  await expect(page).toHaveScreenshot({ clip: CLIPPING_AREA });
});
