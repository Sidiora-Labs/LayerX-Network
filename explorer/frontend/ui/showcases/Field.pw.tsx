import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Field from './Field';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="field"><Field/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
