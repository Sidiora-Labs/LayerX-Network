import type { BrowserContext } from '@playwright/test';
import React from 'react';

import { contextWithAuth } from 'playwright/fixtures/auth';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import Burger from './Burger';

test.use({ viewport: devices['iPhone 13 Pro'].viewport });

const hooksConfig = {
  router: {
    route: '/blocks',
    query: { id: '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859' },
    pathname: '/blocks',
  },
};

test('base view', async({ render, page }) => {
  const component = await render(<Burger/>, { hooksConfig });

  await component.getByRole('button', { name: 'Menu button' }).click();
  await expect(page).toHaveScreenshot();
});

test.describe('dark mode', () => {
  test.use({ colorScheme: 'dark' });

  test('base view', async({ render, page }) => {
    const component = await render(<Burger/>, { hooksConfig });

    await component.getByRole('button', { name: 'Menu button' }).click();
    await expect(page).toHaveScreenshot();
  });
});

test('submenu', async({ render, page }) => {
  const component = await render(<Burger/>, { hooksConfig });

  await component.getByRole('button', { name: 'Menu button' }).click();
  await page.locator('div[aria-label="Blockchain link group"]').click();
  await expect(page).toHaveScreenshot();
});

const authTest = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

authTest.describe('auth', () => {
  authTest.use({ viewport: { width: devices['iPhone 13 Pro'].viewport.width, height: 800 } });

  authTest('base view', async({ render, page }) => {
    const component = await render(<Burger/>, { hooksConfig });

    await component.getByRole('button', { name: 'Menu button' }).click();
    await expect(page).toHaveScreenshot();
  });
});

const promoBannerTest = (type: 'text' | 'image') => {
  test.describe(`with promo banner (${ type })`, () => {
    const darkModeRule = type === 'text' ? '+@dark-mode' : '';

    test.beforeEach(async({ mockEnvs, mockAssetResponse }) => {
      await mockEnvs(type === 'text' ? ENVS_MAP.navigationPromoBannerText : ENVS_MAP.navigationPromoBannerImage);
      await mockAssetResponse('http://localhost:3000/image.svg', './playwright/mocks/image_svg.svg');
      await mockAssetResponse('http://localhost:3000/image_s.jpg', './playwright/mocks/image_s.jpg');
      await mockAssetResponse('http://localhost:3000/image_md.jpg', './playwright/mocks/image_md.jpg');
    });

    test(`${ darkModeRule }`, async({ render, page }) => {
      const component = await render(<Burger/>);
      await component.getByRole('button', { name: 'Menu button' }).click();
      await expect(page).toHaveScreenshot();
    });
  });
};

promoBannerTest('text');
promoBannerTest('image');
