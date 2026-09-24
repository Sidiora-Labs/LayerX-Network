import React from 'react';

import * as poolMock from 'mocks/pools/pool';
import { test, expect } from 'playwright/lib';

import PoolEntity from './PoolEntity';

test.use({ viewport: { width: 300, height: 100 } });

test('with icons +@dark-mode', async({ render, mockAssetResponse }) => {
  await mockAssetResponse('https://localhost:3000/utia.jpg', './playwright/mocks/image_s.jpg');
  await mockAssetResponse('https://localhost:3000/secondary_utia.jpg', './playwright/mocks/image_md.jpg');
  const component = await render(
    <PoolEntity
      pool={ poolMock.base }
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('no icons +@dark-mode', async({ render }) => {
  const component = await render(
    <PoolEntity
      pool={ poolMock.noIcons }
    />,
  );

  await expect(component).toHaveScreenshot();
});
