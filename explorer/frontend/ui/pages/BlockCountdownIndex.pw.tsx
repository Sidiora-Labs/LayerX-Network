import React from 'react';

import { test, expect } from 'playwright/lib';

import BlockCountdownIndex from './BlockCountdownIndex';

test('base view +@mobile', async({ render }) => {
  const component = await render(<BlockCountdownIndex/>);
  await expect(component).toHaveScreenshot();
});
