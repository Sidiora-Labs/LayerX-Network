import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Tabs from './Tabs';

test('default +@dark-mode', async({ render, page }) => {
  const component = await render(<TabsRoot defaultValue="tabs"><Tabs/></TabsRoot>);
  await component.getByLabel('Open tabs menu').click();
  await page.getByRole('dialog').getByText('Lending50+').hover();
  await expect(page).toHaveScreenshot();
});

test('color theme overrides +@dark-mode', async({ render, mockEnvs, page }) => {
  await mockEnvs(ENVS_MAP.colorThemeOverrides);
  const component = await render(<TabsRoot defaultValue="tabs"><Tabs/></TabsRoot>);
  await component.getByLabel('Open tabs menu').click();
  await page.getByRole('dialog').getByText('Lending50+').hover();
  await expect(page).toHaveScreenshot();
});
