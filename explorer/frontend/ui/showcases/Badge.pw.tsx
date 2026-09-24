import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Badge from './Badge';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="badge"><Badge/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
