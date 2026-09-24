import { Box } from '@chakra-ui/react';
import React from 'react';

import * as tokens from 'mocks/tokens/tokenInfo';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import Tokens from './Tokens';

test.beforeEach(async({ mockTextAd, mockAssetResponse }) => {
  await mockTextAd();
  await mockAssetResponse(tokens.tokenInfoERC20a.icon_url as string, './playwright/mocks/image_svg.svg');
});

const allTokens = {
  items: [
    tokens.tokenInfoERC20a, tokens.tokenInfoERC20b, tokens.tokenInfoERC20c, tokens.tokenInfoERC20d,
    tokens.tokenInfoERC721a, tokens.tokenInfoERC721b, tokens.tokenInfoERC721c,
    tokens.tokenInfoERC1155a, tokens.tokenInfoERC1155b, tokens.tokenInfoERC1155WithoutName,
  ],
  next_page_params: {
    holders_count: 1,
    items_count: 1,
    name: 'a',
    market_cap: '0',
  },
};

// FIXME: test is flaky, screenshot in docker container is different from local
test.skip('base view +@mobile +@dark-mode', async({ render, mockApiResponse }) => {

  await mockApiResponse('general:tokens', allTokens);

  const component = await render(
    <div>
      <Box h={{ base: '134px', lg: 6 }}/>
      <Tokens/>
    </div>,
  );

  await expect(component).toHaveScreenshot();
});

test('with search +@mobile +@dark-mode', async({ page, render, mockApiResponse }) => {
  const filteredTokens = {
    items: [
      tokens.tokenInfoERC20a, tokens.tokenInfoERC20b, tokens.tokenInfoERC20c,
    ],
    next_page_params: null,
  };

  await mockApiResponse('general:tokens', allTokens);
  const filteredTokensApiUrl = await mockApiResponse('general:tokens', filteredTokens, { queryParams: { q: 'foo' } });

  const component = await render(
    <div>
      <Box h={{ base: '134px', lg: 6 }}/>
      <Tokens/>
    </div>,
  );

  const requestPromise = page.waitForRequest(filteredTokensApiUrl);
  await component.getByRole('textbox', { name: 'Token name or symbol' }).focus();
  await component.getByRole('textbox', { name: 'Token name or symbol' }).fill('foo');
  await component.getByRole('textbox', { name: 'Token name or symbol' }).blur();

  await requestPromise;
  await expect(component).toHaveScreenshot({ maxDiffPixels: 20 });
});

test.describe('bridged tokens', () => {
  const bridgedTokens = {
    items: [
      tokens.bridgedTokenA,
      tokens.bridgedTokenB,
      tokens.bridgedTokenC,
    ],
    next_page_params: {
      holders_count: 1,
      items_count: 1,
      name: 'a',
      market_cap: null,
    },
  };
  const bridgedFilteredTokens = {
    items: [
      tokens.bridgedTokenC,
    ],
    next_page_params: null,
  };
  const hooksConfig = {
    router: {
      query: { tab: 'bridged' },
    },
  };

  test('base view', async({ render, page, mockApiResponse, mockEnvs }) => {
    await mockEnvs(ENVS_MAP.bridgedTokens);
    await mockApiResponse('general:tokens_bridged', bridgedTokens);
    const bridgedFilteredTokensApiUrl = await mockApiResponse('general:tokens_bridged', bridgedFilteredTokens, { queryParams: { chain_ids: '99' } });

    const component = await render(
      <div>
        <Box h={{ base: '134px', lg: 6 }}/>
        <Tokens/>
      </div>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();

    await component.getByRole('button', { name: /filter/i }).click();
    const requestPromise = page.waitForRequest(bridgedFilteredTokensApiUrl);
    await page.locator('label').filter({ hasText: /poa/i }).click();
    await page.click('body');

    await requestPromise;

    await expect(component).toHaveScreenshot();
  });
});
