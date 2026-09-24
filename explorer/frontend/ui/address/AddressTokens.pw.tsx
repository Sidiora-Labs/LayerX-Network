import { Box } from '@chakra-ui/react';
import React from 'react';

import type { AddressTokenBalance, AddressTokensResponse } from 'types/api/address';

import * as addressMock from 'mocks/address/address';
import * as tokensMock from 'mocks/address/tokens';
import * as tokenInfo from 'mocks/tokens/tokenInfo';
import * as tokenInstance from 'mocks/tokens/tokenInstance';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect, devices } from 'playwright/lib';

import AddressTokens from './AddressTokens';

const ADDRESS_HASH = addressMock.validator.hash;

const nextPageParams = {
  items_count: 50,
  token_name: 'aaa',
  token_type: 'ERC-20' as const,
  value: 1,
  fiat_value: '1',
};

test.beforeEach(async({ mockApiResponse }) => {
  const response20: AddressTokensResponse = {
    items: [ tokensMock.erc20a, tokensMock.erc20b, tokensMock.erc20c, tokensMock.erc20d ],
    next_page_params: nextPageParams,
  };
  const response721: AddressTokensResponse = {
    items: [ tokensMock.erc721a, tokensMock.erc721b, tokensMock.erc721c ],
    next_page_params: nextPageParams,
  };
  const response1155: AddressTokensResponse = {
    items: [ tokensMock.erc1155a, tokensMock.erc1155b ],
    next_page_params: nextPageParams,
  };
  const response404: AddressTokensResponse = {
    items: [ tokensMock.erc404a, tokensMock.erc404b ],
    next_page_params: nextPageParams,
  };

  await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: ADDRESS_HASH } });
  await mockApiResponse('general:address_tokens', response20, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' } });
  await mockApiResponse('general:address_tokens', response721, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' } });
  await mockApiResponse('general:address_tokens', response1155, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' } });
  await mockApiResponse('general:address_tokens', response404, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' } });
  await mockApiResponse('general:address_nfts', tokensMock.nfts, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: [] } });
  await mockApiResponse('general:address_collections', tokensMock.collections, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: [] } });
});

test('erc20 +@dark-mode', async({ render }) => {
  const hooksConfig = {
    router: {
      query: { hash: ADDRESS_HASH, tab: 'tokens_erc20' },
      isReady: true,
    },
  };

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokens/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});

test('collections +@dark-mode', async({ render }) => {
  const hooksConfig = {
    router: {
      query: { hash: ADDRESS_HASH, tab: 'tokens_nfts' },
      isReady: true,
    },
  };

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokens/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});

test('nfts +@dark-mode', async({ render, mockAssetResponse }) => {
  await mockAssetResponse(tokenInstance.base.image_url as string, './playwright/mocks/image_s.jpg');

  const hooksConfig = {
    router: {
      query: { hash: ADDRESS_HASH, tab: 'tokens_nfts' },
      isReady: true,
    },
  };

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokens/>
    </Box>,
    { hooksConfig },
  );

  await component.getByText('List').click();

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('erc20', async({ render }) => {
    const hooksConfig = {
      router: {
        query: { hash: ADDRESS_HASH, tab: 'tokens_erc20' },
        isReady: true,
      },
    };

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokens/>
      </Box>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();
  });

  test('nfts', async({ render, mockAssetResponse, page }) => {
    await mockAssetResponse(tokenInstance.base.image_url as string, './playwright/mocks/image_s.jpg');

    const hooksConfig = {
      router: {
        query: { hash: ADDRESS_HASH, tab: 'tokens_nfts' },
        isReady: true,
      },
    };

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokens/>
      </Box>,
      { hooksConfig },
    );

    await component.locator('button').filter({ hasText: 'List' }).click();
    await page.mouse.move(0, 0);
    await page.mouse.click(0, 0);

    await expect(component).toHaveScreenshot();
  });

  test('collections', async({ render }) => {
    const hooksConfig = {
      router: {
        query: { hash: ADDRESS_HASH, tab: 'tokens_nfts' },
        isReady: true,
      },
    };

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokens/>
      </Box>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();
  });
});

test.describe('update balances via socket', () => {
  test.describe.configure({ mode: 'serial' });

  test('base flow', async({ render, page, createSocket, mockApiResponse }) => {
    test.slow();

    const hooksConfig = {
      router: {
        query: { hash: ADDRESS_HASH, tab: 'tokens_erc20' },
        isReady: true,
      },
    };

    const response20 = {
      items: [ tokensMock.erc20a, tokensMock.erc20b ],
      next_page_params: null,
    };
    const response721 = {
      items: [ tokensMock.erc721a, tokensMock.erc721b ],
      next_page_params: null,
    };
    const response1155 = {
      items: [ tokensMock.erc1155a ],
      next_page_params: null,
    };
    const response404 = {
      items: [ tokensMock.erc404a ],
      next_page_params: null,
    };

    const erc20ApiUrl = await mockApiResponse('general:address_tokens', response20, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' } });
    const erc721ApiUrl = await mockApiResponse('general:address_tokens', response721, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' } });
    const erc1155ApiUrl = await mockApiResponse(
      'general:address_tokens',
      response1155,
      { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' } },
    );
    const erc404ApiUrl = await mockApiResponse(
      'general:address_tokens',
      response404,
      { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' } },
    );

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokens/>
      </Box>,
      { hooksConfig },
      { withSocket: true },
    );

    await page.waitForResponse(erc20ApiUrl);
    await page.waitForResponse(erc721ApiUrl);
    await page.waitForResponse(erc1155ApiUrl);
    await page.waitForResponse(erc404ApiUrl);

    await expect(component).toHaveScreenshot();

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ ADDRESS_HASH.toLowerCase() }`);
    socketServer.sendMessage(socket, channel, 'updated_token_balances_erc_20', {
      overflow: false,
      token_balances: [
        {
          ...tokensMock.erc20a,
          token: {
            ...tokensMock.erc20a.token,
            exchange_rate: '0.01',
          },
        },
        {
          ...tokensMock.erc20c,
          value: '9852000000000000',
          token: {
            ...tokensMock.erc20c.token,
            address_hash: '0xE2cf36D00C57e01371b94B4206ae2CF841931Adc',
            name: 'Tether USD',
            symbol: 'USDT',
          },
        },
      ],
    });
    socketServer.sendMessage(socket, channel, 'updated_token_balances_erc_721', {
      overflow: false,
      token_balances: [
        {
          ...tokensMock.erc721c,
          token: {
            ...tokensMock.erc721c.token,
            exchange_rate: '20',
          },
        },
      ],
    });

    await expect(component).toHaveScreenshot();
  });

  test('custom token ZRC-2', async({ render, page, createSocket, mockApiResponse, mockEnvs }) => {
    test.slow();

    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_ADDITIONAL_TOKEN_TYPES', '[{"id":"ZRC-2","name":"ZRC-2"}]' ],
    ]);

    const hooksConfig = {
      router: {
        query: { hash: ADDRESS_HASH, tab: 'tokens_erc20' },
        isReady: true,
      },
    };

    const responseErc20AndZrc2 = {
      items: [ tokensMock.erc20a ],
      next_page_params: null,
    };
    const responseZrc2 = {
      items: [],
      next_page_params: null,
    };

    const erc20AndZrc2ApiUrl = await mockApiResponse(
      'general:address_tokens',
      responseErc20AndZrc2,
      { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: [ 'ERC-20', 'ZRC-2' ] } },
    );
    const zrc2ApiUrl = await mockApiResponse(
      'general:address_tokens',
      responseZrc2,
      { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ZRC-2' } },
    );

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokens/>
      </Box>,
      { hooksConfig },
      { withSocket: true },
    );

    await page.waitForResponse(erc20AndZrc2ApiUrl);
    await page.waitForResponse(zrc2ApiUrl);

    await expect(component).toHaveScreenshot();

    const zrc2TokenBalance: AddressTokenBalance = {
      ...tokensMock.erc20a,
      token: {
        ...tokensMock.erc20a.token,
        // custom token type enabled via NEXT_PUBLIC_NETWORK_ADDITIONAL_TOKEN_TYPES
        type: 'ZRC-2',
        address_hash: '0x00000000000000000000000000000000000000f2',
        name: 'ZRC-2 Token',
        symbol: 'ZRC2',
        exchange_rate: '0.02',
      } as unknown as AddressTokenBalance['token'],
      value: '1234500000000000000',
    };

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ ADDRESS_HASH.toLowerCase() }`);
    socketServer.sendMessage(socket, channel, 'updated_token_balances_zrc_2', {
      overflow: false,
      token_balances: [ zrc2TokenBalance ],
    });

    await expect(component).toHaveScreenshot();
  });
});

test('native token', async({ render, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_ADDRESS_NATIVE_TOKEN_ADDRESS', tokenInfo.tokenInfoERC20c.address_hash ],
  ]);
  const hooksConfig = {
    router: {
      query: { hash: ADDRESS_HASH, tab: 'tokens_erc20' },
      isReady: true,
    },
  };

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokens/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});
