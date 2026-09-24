import type { Locator } from '@playwright/test';
import React from 'react';

import { test, expect } from 'playwright/lib';

import NetworkLogo from './NetworkLogo';

const LOGO_URL = 'https://localhost:3000/my-logo.png';

test.use({ viewport: { width: 120, height: 30 } });

test.describe('placeholder logo', () => {
  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_LOGO', '' ],
    ]);
  });

  test('+@dark-mode', async({ render }) => {
    const component = await render(<NetworkLogo/>);

    await expect(component.locator('a')).toHaveScreenshot();
  });
});

test.describe('custom logo', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockConfigResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_LOGO', LOGO_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_LOGO', LOGO_URL, './playwright/mocks/network-logo.svg', true);
    component = await render(<NetworkLogo/>);
  });

  test('+@dark-mode', async() => {
    await expect(component.locator('a')).toHaveScreenshot();
  });
});

test.describe('custom logo with dark option -@default +@dark-mode', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockConfigResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_LOGO', LOGO_URL ],
      [ 'NEXT_PUBLIC_NETWORK_LOGO_DARK', LOGO_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_LOGO', LOGO_URL, './playwright/mocks/image_long.jpg', true);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_LOGO_DARK', LOGO_URL, './playwright/mocks/image_long.jpg', true);

    component = await render(<NetworkLogo/>);
  });

  test('base view', async() => {
    await expect(component.locator('a')).toHaveScreenshot();
  });
});
