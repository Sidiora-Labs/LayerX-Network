import React from 'react';

import * as mocks from 'mocks/account/verifiedAddresses';
import { test, expect } from 'playwright/lib';

import TokenInfoForm from './TokenInfoForm';

test.beforeEach(async({ mockApiResponse, mockAssetResponse }) => {
  await mockApiResponse('admin:token_info_applications_config', mocks.TOKEN_INFO_FORM_CONFIG, { pathParams: { chainId: '1' } });
  await mockAssetResponse(mocks.TOKEN_INFO_APPLICATION_BASE.iconUrl, './playwright/mocks/image_md.jpg');
});

test('base view +@mobile +@dark-mode', async({ render }) => {
  const props = {
    address: mocks.VERIFIED_ADDRESS.ITEM_1.contractAddress,
    tokenName: 'Test Token (TT)',
    application: mocks.TOKEN_INFO_APPLICATION.APPROVED,
    onSubmit: () => {},
  };
  const component = await render(<TokenInfoForm { ...props }/>);
  await expect(component).toHaveScreenshot();
});

test('status IN_PROCESS', async({ render }) => {
  const props = {
    address: mocks.VERIFIED_ADDRESS.ITEM_1.contractAddress,
    tokenName: 'Test Token (TT)',
    application: mocks.TOKEN_INFO_APPLICATION.IN_PROCESS,
    onSubmit: () => {},
  };
  const component = await render(<TokenInfoForm { ...props }/>);
  await expect(component).toHaveScreenshot();
});
