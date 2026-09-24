import { Box } from '@chakra-ui/react';
import React from 'react';

import { tokenHoldersERC20, tokenHoldersERC1155 } from 'mocks/tokens/tokenHolders';
import { tokenInfo, tokenInfoERC1155a } from 'mocks/tokens/tokenInfo';
import { test, expect } from 'playwright/lib';

import TokenHoldersTable from './TokenHoldersTable';

test('base view without IDs', async({ render }) => {
  const component = await render(
    <Box pt="128px">
      <TokenHoldersTable data={ tokenHoldersERC20.items } token={ tokenInfo } top={ 88 }/>
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});

test('base view with IDs', async({ render }) => {
  const component = await render(
    <Box pt="128px">
      <TokenHoldersTable data={ tokenHoldersERC1155.items } token={ tokenInfoERC1155a } top={ 88 }/>
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});
