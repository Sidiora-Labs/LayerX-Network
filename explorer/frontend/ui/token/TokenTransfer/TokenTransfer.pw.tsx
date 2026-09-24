import { Box } from '@chakra-ui/react';
import React from 'react';

import { tokenInfoERC20a, tokenInfoERC721a, tokenInfoERC1155a } from 'mocks/tokens/tokenInfo';
import * as tokenInstanceMock from 'mocks/tokens/tokenInstance';
import * as tokenTransferMock from 'mocks/tokens/tokenTransfer';
import { test, expect } from 'playwright/lib';

import TokenTransfer from './TokenTransfer';

test('erc20 +@mobile', async({ render }) => {
  const component = await render(
    <Box pt={{ base: '134px', lg: '100px' }}>
      <TokenTransfer

        // @ts-ignore:
        transfersQuery={{
          data: {
            items: [ tokenTransferMock.erc20 ],
            next_page_params: null,
          },

          // @ts-ignore:
          pagination: { page: 1, isVisible: true },
        }}
        // @ts-ignore:
        tokenQuery={{
          data: tokenInfoERC20a,
        }}
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});

test('erc721 +@mobile', async({ render, mockAssetResponse }) => {
  await mockAssetResponse(tokenInstanceMock.base.image_url as string, './playwright/mocks/image_s.jpg');
  const component = await render(
    <Box pt={{ base: '134px', lg: '100px' }}>
      <TokenTransfer

        // @ts-ignore:
        transfersQuery={{
          data: {
            items: [ tokenTransferMock.erc721 ],
            next_page_params: null,
          },

          // @ts-ignore:
          pagination: { page: 1, isVisible: true },
        }}
        // @ts-ignore:
        tokenQuery={{
          data: tokenInfoERC721a,
        }}
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});

test('erc1155 +@mobile', async({ render }) => {
  const component = await render(
    <Box pt={{ base: '134px', lg: '100px' }}>
      <TokenTransfer

        // @ts-ignore:
        transfersQuery={{
          data: {
            items: [
              tokenTransferMock.erc1155A,
              tokenTransferMock.erc1155B,
              tokenTransferMock.erc1155C,
              tokenTransferMock.erc1155D,
            ],
            next_page_params: null,
          },

          // @ts-ignore:
          pagination: { page: 1, isVisible: true },
        }}
        // @ts-ignore:
        tokenQuery={{
          data: tokenInfoERC1155a,
        }}
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});
