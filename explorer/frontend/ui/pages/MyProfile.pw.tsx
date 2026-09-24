import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { test as base, expect } from 'playwright/lib';

import MyProfile from './MyProfile';

const test = base.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

test('without address', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:user_info', profileMock.base);
  const component = await render(<MyProfile/>);

  await expect(component).toHaveScreenshot();
});

test('without email', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:user_info', profileMock.withoutEmail);
  const component = await render(<MyProfile/>);

  await expect(component).toHaveScreenshot();
});
