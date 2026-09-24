import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Link from './Link';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="link"><Link/></TabsRoot>);

  const variantsSection = component.locator('section[title="variants"]');
  await expect(variantsSection).toHaveScreenshot();

  const loadingSection = component.locator('section[title="loading"]');
  await expect(loadingSection).toHaveScreenshot();
});

test('color theme overrides +@dark-mode', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.colorThemeOverrides);
  const component = await render(<TabsRoot defaultValue="link"><Link/></TabsRoot>);

  const variantsSection = component.locator('section[title="variants"]');
  await expect(variantsSection).toHaveScreenshot();
});
