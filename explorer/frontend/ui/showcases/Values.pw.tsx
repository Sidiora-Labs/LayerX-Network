import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Values from './Values';

test('default', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="values"><Values/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
