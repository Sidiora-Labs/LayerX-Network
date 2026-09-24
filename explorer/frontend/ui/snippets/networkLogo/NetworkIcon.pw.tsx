import type { Locator } from '@playwright/test';
import React from 'react';

import { test, expect } from 'playwright/lib';

import NetworkIcon from './NetworkIcon';

const ICON_URL = 'https://localhost:3000/my-icon.png';

test.use({ viewport: { width: 30, height: 30 } });

test.describe('placeholder icon', () => {
  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_ICON', '' ],
    ]);
  });

  test('+@dark-mode', async({ render }) => {
    const component = await render(<NetworkIcon/>);

    await expect(component).toHaveScreenshot();
  });
});

test.describe('custom icon', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockConfigResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_ICON', ICON_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_ICON', ICON_URL, './playwright/mocks/image_svg.svg', true);
    component = await render(<NetworkIcon/>);
  });

  test('+@dark-mode', async() => {
    await expect(component).toHaveScreenshot();
  });
});

test.describe('custom icon with dark option -@default +@dark-mode', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockConfigResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_NETWORK_ICON', ICON_URL ],
      [ 'NEXT_PUBLIC_NETWORK_ICON_DARK', ICON_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_ICON', ICON_URL, './playwright/mocks/image_s.jpg', true);
    await mockConfigResponse('NEXT_PUBLIC_NETWORK_ICON_DARK', ICON_URL, './playwright/mocks/image_s.jpg', true);

    component = await render(<NetworkIcon/>);
  });

  test('base view', async() => {
    await expect(component).toHaveScreenshot();
  });
});
