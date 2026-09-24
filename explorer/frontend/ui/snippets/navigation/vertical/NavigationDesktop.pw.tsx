import { Box, Flex } from '@chakra-ui/react';
import type { BrowserContext, Locator } from '@playwright/test';
import React from 'react';

import config from 'configs/app';
import * as cookies from 'lib/cookies';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import NavigationDesktop from './NavigationDesktop';

const hooksConfig = {
  router: {
    route: '/blocks',
    query: { id: '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859' },
    pathname: '/blocks',
  },
};

test.describe('no auth', () => {
  let component: Locator;

  test.beforeEach(async({ render }) => {
    component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );
  });

  test('+@dark-mode', async({ page }) => {
    await page.locator('a[aria-label="Link to main page"]').last().hover();
    await expect(component).toHaveScreenshot();
  });

  test.describe('xl screen', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('+@dark-mode', async({ page }) => {
      await page.locator('a[aria-label="Link to main page"]').first().hover();
      await expect(component).toHaveScreenshot();
    });
  });
});

const authTest = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
});

// FIXME: at the moment, in the docker container playwright make screenshot before the page is completely loaded
// I cannot figure out the reason, so I skip this test for now
authTest.describe.skip('auth', () => {
  let component: Locator;

  authTest.beforeEach(async({ render }) => {
    component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );
  });

  authTest('+@dark-mode', async() => {
    await expect(component).toHaveScreenshot();
  });

  authTest.describe('xl screen', () => {
    authTest.use({ viewport: pwConfig.viewport.xl });

    authTest('+@dark-mode', async() => {
      await expect(component).toHaveScreenshot();
    });
  });
});

test.describe('with tooltips', () => {
  test.use({ viewport: pwConfig.viewport.xl });

  test('base view', async({ render, page }) => {
    const component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );

    await component.locator('header').hover();
    await page.locator('svg[aria-label="Expand/Collapse menu"]').click();
    await page.locator('a[aria-label="Dapps link"]').hover();

    await expect(component).toHaveScreenshot();
  });
});

test.describe('with submenu', () => {
  let component: Locator;

  test.beforeEach(async({ render, page }) => {
    component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );
    await page.locator('div[aria-label="Blockchain link group"]').hover();
  });

  test('base view', async() => {
    await expect(component).toHaveScreenshot();
  });

  test.describe('xl screen', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('base view', async() => {
      await expect(component).toHaveScreenshot();
    });
  });
});

const noSideBarCookieTest = test.extend<{ context: BrowserContext }>({
  context: ({ context }, use) => {
    context.addCookies([ { name: cookies.NAMES.NAV_BAR_COLLAPSED, value: 'false', domain: config.app.host, path: '/' } ]);
    use(context);
  },
});

noSideBarCookieTest.describe('cookie set to false', () => {
  let component: Locator;

  noSideBarCookieTest.beforeEach(async({ render }) => {
    component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );
  });

  noSideBarCookieTest('', async() => {
    const chainIcon = component.getByLabel('Network icon placeholder');
    await expect(chainIcon).toBeHidden();
  });

  noSideBarCookieTest.describe('xl screen', () => {
    noSideBarCookieTest.use({ viewport: pwConfig.viewport.xl });

    noSideBarCookieTest('', async() => {
      const chainIcon = component.getByLabel('Network icon placeholder');
      await expect(chainIcon).toBeHidden();
    });
  });
});

const sideBarCookieTest = test.extend<{ context: BrowserContext }>({
  context: ({ context }, use) => {
    context.addCookies([ { name: cookies.NAMES.NAV_BAR_COLLAPSED, value: 'true', domain: config.app.host, path: '/' } ]);
    use(context);
  },
});

sideBarCookieTest.describe('cookie set to true', () => {
  sideBarCookieTest('navbar is collapsed', async({ render }) => {
    const component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );

    const chainIcon = component.getByLabel('Network icon placeholder');
    await expect(chainIcon).toBeVisible();
  });
});

test('hover +@dark-mode', async({ render }) => {
  const component = await render(
    <Flex w="100%" minH="100vh" alignItems="stretch">
      <NavigationDesktop/>
      <Box bgColor="lightpink" w="100%"/>
    </Flex>,
    { hooksConfig },
  );

  await component.locator('header').hover();
  await expect(component).toHaveScreenshot();
});

test.describe('hover xl screen', () => {
  test.use({ viewport: pwConfig.viewport.xl });

  test('+@dark-mode', async({ render }) => {
    const component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );

    await component.locator('header').hover();
    await expect(component).toHaveScreenshot();
  });
});

test.describe('with highlighted routes', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockEnvs }) => {
    await mockEnvs(ENVS_MAP.navigationHighlightedRoutes);

    component = await render(
      <Flex w="100%" minH="100vh" alignItems="stretch">
        <NavigationDesktop/>
        <Box bgColor="lightpink" w="100%"/>
      </Flex>,
      { hooksConfig },
    );
  });

  test('+@dark-mode', async({ page }) => {
    await page.locator('a[aria-label="Link to main page"]').last().hover();
    await expect(component).toHaveScreenshot();
  });

  test('with submenu', async({ page }) => {
    await page.locator('div[aria-label="Blockchain link group"]').hover();
    await expect(component).toHaveScreenshot();
  });

  test.describe('xl screen', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('+@dark-mode', async({ page }) => {
      await page.locator('a[aria-label="Link to main page"]').first().hover();
      await expect(component).toHaveScreenshot();
    });
  });
});

const promoBannerTest = (type: 'text' | 'image') => {
  test.describe(`with promo banner (${ type })`, () => {
    let component: Locator;
    const darkModeRule = type === 'text' ? '+@dark-mode' : '';
    const imageAltText = type === 'text' ? 'Promo banner icon' : 'Promo banner small';

    test.beforeEach(async({ render, mockEnvs, mockAssetResponse }) => {
      await mockEnvs(type === 'text' ? ENVS_MAP.navigationPromoBannerText : ENVS_MAP.navigationPromoBannerImage);
      await mockAssetResponse('http://localhost:3000/image.svg', './playwright/mocks/image_svg.svg');
      await mockAssetResponse('http://localhost:3000/image_s.jpg', './playwright/mocks/image_s.jpg');
      await mockAssetResponse('http://localhost:3000/image_md.jpg', './playwright/mocks/image_md.jpg');

      component = await render(
        <Flex w="100%" minH="100vh" alignItems="stretch">
          <NavigationDesktop/>
          <Box bgColor="lightpink" w="100%"/>
        </Flex>,
        { hooksConfig },
      );

      await component.waitFor({ state: 'visible' });
    });

    test(`${ darkModeRule }`, async({ page }) => {
      await page.locator('a[aria-label="Link to main page"]').last().hover();
      await expect(component).toHaveScreenshot();
    });

    test('with tooltip', async({ page }) => {
      await page.getByAltText(imageAltText).hover();
      await expect(component).toHaveScreenshot();
    });

    test.describe('xl screen', () => {
      test.use({ viewport: pwConfig.viewport.xl });

      test(`${ darkModeRule }`, async({ page }) => {
        await page.locator('a[aria-label="Link to main page"]').first().hover();
        await expect(component).toHaveScreenshot();
      });
    });
  });
};

promoBannerTest('text');
promoBannerTest('image');
