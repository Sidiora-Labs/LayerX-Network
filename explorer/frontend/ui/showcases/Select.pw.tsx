import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Select from './Select';

test('default +@dark-mode', async({ render, page }) => {
  const component = await render(<TabsRoot defaultValue="select"><Select/></TabsRoot>);
  await expect(component).toHaveScreenshot();

  const asyncSection = component.locator('section').filter({ hasText: 'With search (async)' });
  const asyncSelect = asyncSection.getByRole('combobox');
  await asyncSelect.click();

  const lastOption = page.getByRole('option', { name: 'Svelte' });
  await lastOption.hover();

  await expect(page).toHaveScreenshot({ clip: { x: 150, y: 150, width: 400, height: 400 } });
});

test('color theme overrides +@dark-mode', async({ render, mockEnvs, page }) => {
  await mockEnvs(ENVS_MAP.colorThemeOverrides);
  const component = await render(<TabsRoot defaultValue="select"><Select/></TabsRoot>);
  await expect(component).toHaveScreenshot();

  const asyncSection = component.locator('section').filter({ hasText: 'With search (async)' });
  const asyncSelect = asyncSection.getByRole('combobox');
  await asyncSelect.click();

  const lastOption = page.getByRole('option', { name: 'Svelte' });
  await lastOption.hover();

  await expect(page).toHaveScreenshot({ clip: { x: 150, y: 150, width: 400, height: 400 } });
});
