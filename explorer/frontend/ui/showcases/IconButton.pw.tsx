import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import IconButton from './IconButton';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="icon-button"><IconButton/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});

test('color theme overrides +@dark-mode', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.colorThemeOverrides);
  const component = await render(<TabsRoot defaultValue="icon-button"><IconButton/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
