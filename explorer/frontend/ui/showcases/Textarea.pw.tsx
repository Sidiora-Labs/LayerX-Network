import React from 'react';

import { test, expect } from 'playwright/lib';
import { TabsRoot } from 'toolkit/chakra/tabs';

import Textarea from './Textarea';

test('default +@dark-mode', async({ render, page }) => {
  const component = await render(<TabsRoot defaultValue="textarea"><Textarea/></TabsRoot>);
  const elements = component.locator('textarea');

  await elements.nth(1).hover();
  await page.mouse.wheel(0, 105);

  await elements.nth(2).hover();
  await page.mouse.wheel(0, 105);

  await elements.nth(3).hover();
  await page.mouse.wheel(0, 105);

  await elements.nth(5).hover();
  await page.mouse.wheel(0, 105);

  await expect(component).toHaveScreenshot();
});
