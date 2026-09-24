import React from 'react';

import buildUrl from 'lib/api/buildUrl';
import * as mocks from 'mocks/account/verifiedAddresses';
import { test, expect } from 'playwright/lib';

import AddressVerificationStepAddress from './AddressVerificationStepAddress';

const CHECK_ADDRESS_URL = buildUrl('contractInfo:address_verification', { chainId: '1', type: ':prepare' });

test('base view', async({ render, page }) => {
  await page.route(CHECK_ADDRESS_URL, (route) => route.fulfill({
    status: 200,
    json: mocks.ADDRESS_CHECK_RESPONSE.SUCCESS,
  }));
  const props = {
    onContinue: () => {},
    defaultAddress: mocks.VERIFIED_ADDRESS.NEW_ITEM.contractAddress,
  };

  await render(<AddressVerificationStepAddress { ...props }/>);
  await expect(page).toHaveScreenshot();
});

test('SOURCE_CODE_NOT_VERIFIED_ERROR view +@mobile', async({ render, page }) => {
  await page.route(CHECK_ADDRESS_URL, (route) => route.fulfill({
    status: 200,
    json: mocks.ADDRESS_CHECK_RESPONSE.SOURCE_CODE_NOT_VERIFIED_ERROR,
  }));

  const props = {
    onContinue: () => {},
  };

  await render(<AddressVerificationStepAddress { ...props }/>);

  const addressInput = page.getByLabel(/smart contract address/i);
  await addressInput.focus();
  await addressInput.type(mocks.VERIFIED_ADDRESS.NEW_ITEM.contractAddress);
  await page.getByRole('button', { name: /continue/i }).click();

  await expect(page).toHaveScreenshot();
});
