import { Box } from '@chakra-ui/react';
import React from 'react';

import * as chainDataMock from 'mocks/multichain/chains';
import * as tokenMock from 'mocks/tokens/tokenInfo';
import { stableHover } from 'playwright/helpers/stableHover';
import { test, expect } from 'playwright/lib';

import TokenEntity, { Icon } from './TokenEntity';

const variants = [ 'subheading', 'content' ] as const;

test.use({ viewport: { width: 300, height: 100 } });

test.describe('variant', () => {
  variants.forEach((variant) => {
    test(`${ variant }`, async({ render }) => {
      const component = await render(
        <TokenEntity
          token={ tokenMock.tokenInfo }
          variant={ variant }
        />,
      );

      await expect(component).toHaveScreenshot();
    });

    test(`${ variant } with chain data`, async({ render, mockAssetResponse }) => {
      const LOGO_URL = 'https://example.com/logo.png';
      const chainLogoUrl = chainDataMock.chainA.logo;
      await mockAssetResponse(LOGO_URL, './playwright/mocks/image_s.jpg');
      await mockAssetResponse(chainLogoUrl as string, './playwright/mocks/image_svg.svg');

      const component = await render(
        <TokenEntity
          token={{
            type: 'ERC-20',
            name: 'This token is the best token ever',
            symbol: 'DUCK DUCK DUCK',
            address_hash: tokenMock.tokenInfo.address_hash,
            icon_url: LOGO_URL,
            reputation: 'ok',
          }}
          chain={ chainDataMock.chainA }
        />,
      );

      await expect(component).toHaveScreenshot();
    });
  });
});

test('icon in heading variant', async({ render, mockAssetResponse }) => {
  const chainLogoUrl = chainDataMock.chainA.logo;
  await mockAssetResponse(tokenMock.tokenInfo.icon_url as string, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(chainLogoUrl as string, './playwright/mocks/image_svg.svg');

  const component = await render(
    <Icon
      token={ tokenMock.tokenInfo }
      variant="heading"
      chain={ chainDataMock.chainA }
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('with logo, long name and symbol', async({ page, render }) => {
  const LOGO_URL = 'https://example.com/logo.png';
  await page.route(LOGO_URL, (route) => {
    return route.fulfill({
      status: 200,
      path: './playwright/mocks/image_s.jpg',
    });
  });

  await render(
    <TokenEntity
      token={{
        type: 'ERC-20',
        name: 'This token is the best token ever',
        symbol: 'DUCK DUCK DUCK',
        address_hash: tokenMock.tokenInfo.address_hash,
        icon_url: LOGO_URL,
        reputation: 'ok',
      }}
    />,
  );

  await stableHover(page.getByText(/this/i));
  await expect(page).toHaveScreenshot();

  await stableHover(page.getByText(/duc/i));
  await expect(page).toHaveScreenshot();
});

test('scam token', async({ page, render, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_TOKEN_SCAM_TOGGLE_ENABLED', 'true' ],
  ]);

  const LOGO_URL = 'https://example.com/logo.png';
  await page.route(LOGO_URL, (route) => {
    return route.fulfill({
      status: 200,
      path: './playwright/mocks/image_s.jpg',
    });
  });

  await render(
    <TokenEntity
      token={{
        type: 'ERC-20',
        name: 'Duck Token',
        symbol: 'DUCK',
        address_hash: tokenMock.tokenInfo.address_hash,
        icon_url: LOGO_URL,
        reputation: 'scam',
      }}
    />,
  );
  await expect(page).toHaveScreenshot();
});

test('loading', async({ render }) => {
  const component = await render(
    <TokenEntity
      token={ tokenMock.tokenInfo }
      isLoading
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('customization', async({ render }) => {
  const component = await render(
    <Box
      borderWidth="1px"
      borderColor="orange.500"
    >
      <TokenEntity
        token={ tokenMock.tokenInfo }
        p={ 2 }
        maxW="200px"
        borderWidth="1px"
        borderColor="blue.700"
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});
