import React from 'react';

import { test, expect } from 'playwright/lib';

import Utilization from './Utilization';

test.use({ viewport: { width: 100, height: 50 } });

test('green color scheme +@dark-mode', async({ render }) => {
  const component = await render(<Utilization value={ 0.423 }/>);
  await expect(component).toHaveScreenshot();
});

test('gray color scheme +@dark-mode', async({ render }) => {
  const component = await render(<Utilization value={ 0.423 } colorScheme="gray"/>);
  await expect(component).toHaveScreenshot();
});
