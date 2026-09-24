import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import ProgressCircle from './ProgressCircle';

test('default +@dark-mode', async({ render, page }) => {
  await render(<TabsRoot defaultValue="progress-circle"><ProgressCircle/></TabsRoot>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 400 } });
});

test('color theme overrides +@dark-mode', async({ render, mockEnvs, page }) => {
  await mockEnvs(ENVS_MAP.colorThemeOverrides);
  await render(<TabsRoot defaultValue="progress-circle"><ProgressCircle/></TabsRoot>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 400 } });
});
