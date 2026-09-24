import { Box } from '@chakra-ui/react';
import React from 'react';

import { tokenInfoERC721a } from 'mocks/tokens/tokenInfo';
import { base as tokenInstance } from 'mocks/tokens/tokenInstance';
import { test, expect } from 'playwright/lib';

import TokenInventory from './TokenInventory';

test('base view +@mobile', async({ render, mockAssetResponse }) => {

  const item = { ...tokenInstance, image_url: null };

  await mockAssetResponse(tokenInstance.image_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(
    <Box pt={{ base: '134px', lg: 0 }}>
      <TokenInventory

        // @ts-ignore:
        inventoryQuery={{
          data: {
            items: [ tokenInstance, item, item ],
            next_page_params: { unique_token: 1 },
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
