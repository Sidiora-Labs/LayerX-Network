import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { test, expect } from 'playwright/lib';

import AuthModalStory from './AuthModal.pwstory';

test('email login', async({ render, page, mockApiResponse }) => {

  await render(<AuthModalStory flow="email_login"/>);

  await expect(page.getByText('Status: Not authenticated')).toBeVisible();

  await page.getByText('Log in').click();
  await expect(page).toHaveScreenshot();

  // fill email
  await page.getByText('Continue with email').click();
  await page.getByLabel(/email/i).getByPlaceholder(' ').fill('john.doe@example.com');
  await expect(page).toHaveScreenshot();

  // send otp code
  await mockApiResponse('general:auth_send_otp', {} as never);
  await page.getByText('Send a code').click();

  // fill otp code
  await page.getByLabel('pin code 1 of 6').fill('1');
  await page.getByLabel('pin code 2 of 6').fill('2');
  await page.getByLabel('pin code 3 of 6').fill('3');
  await page.getByLabel('pin code 4 of 6').fill('4');
  await page.getByLabel('pin code 5 of 6').fill('5');
  await page.getByLabel('pin code 6 of 6').fill('6');
  // make sure that the focus is removed from the button
  await page.getByText('Confirmation code').click();
  await expect(page).toHaveScreenshot();

  // submit otp code
  await mockApiResponse('general:auth_confirm_otp', profileMock.base as never);
  await page.getByText('Submit').click();
  // make sure that the focus is removed from the button
  await page.getByText('Congrats').click();
  await expect(page).toHaveScreenshot();

  await page.getByLabel('Close').click();
  await expect(page.getByText('Status: Authenticated')).toBeVisible();
});

const linkEmailTest = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

linkEmailTest('link email to account', async({ render, page, mockApiResponse }) => {
  await mockApiResponse('general:user_info', profileMock.base);
  await render(<AuthModalStory flow="email_link"/>);

  await expect(page.getByText('Status: Authenticated')).toBeVisible();

  // fill email
  await page.getByText('Link email').click();
  await page.getByLabel(/email/i).getByPlaceholder(' ').fill('john.doe@example.com');
  await expect(page).toHaveScreenshot();

  // send and fill otp code
  await mockApiResponse('general:auth_send_otp', {} as never);
  await page.getByText('Send a code').click();
  await page.getByLabel('pin code 1 of 6').fill('1');
  await page.getByLabel('pin code 2 of 6').fill('2');
  await page.getByLabel('pin code 3 of 6').fill('3');
  await page.getByLabel('pin code 4 of 6').fill('4');
  await page.getByLabel('pin code 5 of 6').fill('5');
  await page.getByLabel('pin code 6 of 6').fill('6');
  await mockApiResponse('general:auth_link_email', profileMock.base as never);
  await page.getByText('Submit').click();

  await expect(page).toHaveScreenshot();
});
