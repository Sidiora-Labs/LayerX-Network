import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Alert from './Alert';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="alert"><Alert/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
