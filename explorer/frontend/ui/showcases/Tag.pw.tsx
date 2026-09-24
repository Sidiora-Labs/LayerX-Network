import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Tag from './Tag';

test('default +@dark-mode', async({ render }) => {
  const component = await render(<TabsRoot defaultValue="tag"><Tag/></TabsRoot>);
  await expect(component).toHaveScreenshot();
});
