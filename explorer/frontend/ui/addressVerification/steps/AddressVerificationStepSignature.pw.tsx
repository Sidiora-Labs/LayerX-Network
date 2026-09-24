import React from 'react';

import buildUrl from 'lib/api/buildUrl';
import * as mocks from 'mocks/account/verifiedAddresses';
import { test, expect } from 'playwright/lib';

import AddressVerificationStepSignature from './AddressVerificationStepSignature';

const VERIFY_ADDRESS_URL = buildUrl('contractInfo:address_verification', { chainId: '1', type: ':verify' });

test('base view', async({ render, page }) => {
  await page.route(VERIFY_ADDRESS_URL, (route) => route.fulfill({
    status: 200,
    json: mocks.ADDRESS_VERIFY_RESPONSE.SUCCESS,
  }));

  const props = {
    onContinue: () => {},
    noWeb3Provider: true,
    address: mocks.VERIFIED_ADDRESS.NEW_ITEM.contractAddress,
    signingMessage: mocks.ADDRESS_CHECK_RESPONSE.SUCCESS.result.signingMessage,
  };

  await render(<AddressVerificationStepSignature { ...props }/>);
  await expect(page).toHaveScreenshot();
});

test('INVALID_SIGNER_ERROR view +@mobile', async({ render, page }) => {
  await page.route(VERIFY_ADDRESS_URL, (route) => route.fulfill({
    status: 200,
    json: mocks.ADDRESS_VERIFY_RESPONSE.INVALID_SIGNER_ERROR,
  }));

  const props = {
    onContinue: () => {},
    noWeb3Provider: true,
    address: mocks.VERIFIED_ADDRESS.NEW_ITEM.contractAddress,
    ...mocks.ADDRESS_CHECK_RESPONSE.SUCCESS.result,
  };

  await render(<AddressVerificationStepSignature { ...props }/>);

  const signatureInput = page.getByLabel(/signature hash/i);
  await signatureInput.fill(mocks.SIGNATURE);
  await page.getByRole('button', { name: /verify/i }).click();

  await expect(page).toHaveScreenshot({ fullPage: true });
});
