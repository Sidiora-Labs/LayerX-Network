import React from 'react';

import { tokenHoldersERC20, tokenHoldersERC1155 } from 'mocks/tokens/tokenHolders';
import { tokenInfo, tokenInfoERC1155a } from 'mocks/tokens/tokenInfo';
import { test, expect, devices } from 'playwright/lib';

import TokenHoldersList from './TokenHoldersList';

test.use({ viewport: devices['iPhone 13 Pro'].viewport });

test('base view without IDs', async({ render }) => {
  const component = await render(<TokenHoldersList data={ tokenHoldersERC20.items } token={ tokenInfo }/>);
  await expect(component).toHaveScreenshot();
});

test('base view with IDs', async({ render }) => {
  const component = await render(<TokenHoldersList data={ tokenHoldersERC1155.items } token={ tokenInfoERC1155a }/>);
  await expect(component).toHaveScreenshot();
});
