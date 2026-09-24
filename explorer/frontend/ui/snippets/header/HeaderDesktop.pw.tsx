import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import HeaderDesktop from './HeaderDesktop';

test.beforeEach(async({ mockEnvs }) => {
  await mockEnvs([
    ...ENVS_MAP.rewardsService,
  ]);
});

test('default view +@dark-mode', async({ render }) => {
  const component = await render(<HeaderDesktop/>);
  await expect(component).toHaveScreenshot();
});
