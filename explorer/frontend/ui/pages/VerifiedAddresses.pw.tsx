import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as mocks from 'mocks/account/verifiedAddresses';
import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { test as base, expect } from 'playwright/lib';

import VerifiedAddresses from './VerifiedAddresses';

const test = base.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

test.beforeEach(async({ mockAssetResponse }) => {
  await mockAssetResponse(mocks.TOKEN_INFO_APPLICATION_BASE.iconUrl, './playwright/mocks/image_s.jpg');
});

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('contractInfo:verified_addresses', mocks.VERIFIED_ADDRESS_RESPONSE.DEFAULT, { pathParams: { chainId: '1' } });
  await mockApiResponse('admin:token_info_applications', mocks.TOKEN_INFO_APPLICATIONS_RESPONSE.DEFAULT, { pathParams: { chainId: '1', id: undefined } });
  await mockApiResponse('general:user_info', profileMock.base);

  const component = await render(<VerifiedAddresses/>);
  await expect(component).toHaveScreenshot();
});

test('user without email', async({ render, mockApiResponse }) => {
  await mockApiResponse('contractInfo:verified_addresses', mocks.VERIFIED_ADDRESS_RESPONSE.DEFAULT, { pathParams: { chainId: '1' } });
  await mockApiResponse('admin:token_info_applications', mocks.TOKEN_INFO_APPLICATIONS_RESPONSE.DEFAULT, { pathParams: { chainId: '1', id: undefined } });
  await mockApiResponse('general:user_info', profileMock.withoutEmail);

  const component = await render(<VerifiedAddresses/>);

  await expect(component).toHaveScreenshot();
});

test('address verification flow', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('contractInfo:verified_addresses', mocks.VERIFIED_ADDRESS_RESPONSE.DEFAULT, { pathParams: { chainId: '1' } });
  await mockApiResponse('admin:token_info_applications', mocks.TOKEN_INFO_APPLICATIONS_RESPONSE.DEFAULT, { pathParams: { chainId: '1', id: undefined } });
  await mockApiResponse('contractInfo:address_verification', mocks.ADDRESS_CHECK_RESPONSE.SUCCESS as never, { pathParams: { chainId: '1', type: ':prepare' } });
  await mockApiResponse('contractInfo:address_verification', mocks.ADDRESS_VERIFY_RESPONSE.SUCCESS as never, { pathParams: { chainId: '1', type: ':verify' } });
  await mockApiResponse('general:user_info', profileMock.base);

  await render(<VerifiedAddresses/>);

  // open modal
  await page.getByRole('button', { name: /add address/i }).click();

  // fill first step
  const addressInput = page.getByLabel(/smart contract address/i);
  await addressInput.fill(mocks.VERIFIED_ADDRESS.NEW_ITEM.contractAddress);
  await page.getByRole('button', { name: /continue/i }).click();

  // fill second step
  await page.getByText('Sign manually').click();
  const signatureInput = page.getByLabel(/signature hash/i);
  await signatureInput.fill(mocks.SIGNATURE);
  await page.getByRole('button', { name: /verify/i }).click();

  // success screen
  await page.getByRole('button', { name: /view my verified addresses/i }).click();

  await expect(page).toHaveScreenshot();
});

test('application update flow', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('contractInfo:verified_addresses', mocks.VERIFIED_ADDRESS_RESPONSE.DEFAULT, { pathParams: { chainId: '1' } });
  await mockApiResponse('admin:token_info_applications', mocks.TOKEN_INFO_APPLICATIONS_RESPONSE.FOR_UPDATE, { pathParams: { chainId: '1', id: undefined } });
  await mockApiResponse('general:user_info', profileMock.base);
  await mockApiResponse('admin:token_info_applications_config', mocks.TOKEN_INFO_FORM_CONFIG, { pathParams: { chainId: '1' } });

  await mockApiResponse(
    'admin:token_info_applications',
    mocks.TOKEN_INFO_APPLICATION.UPDATED_ITEM as never, // this mock is for PUT request
    { pathParams: { chainId: '1', id: mocks.TOKEN_INFO_APPLICATION.UPDATED_ITEM.id } },
  );

  await render(<VerifiedAddresses/>);

  // open form
  await page.locator('tr').filter({ hasText: 'waiting for update' }).locator('button[aria-label="edit"]').click();

  // change project name
  const addressInput = page.getByLabel(/project name/i);
  await addressInput.fill('New name');

  await page.getByRole('button', { name: /send request/i }).click();

  const locator = page.locator('tr').filter({ hasText: 'in progress' }).filter({ hasText: 'nov 11, 2022' });
  await expect(locator).toBeVisible();
});
