import type { Locator } from '@playwright/test';
import React from 'react';

import * as statsMock from 'mocks/stats/index';
import { test, expect } from 'playwright/lib';

import Stats from './Stats';

test.describe('all items', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockApiResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_HOMEPAGE_STATS', '["total_blocks","average_block_time","total_txs","wallet_addresses","gas_tracker","btc_locked"]' ],
      [ 'NEXT_PUBLIC_STATS_API_HOST', '' ],
    ]);
    await mockApiResponse('general:stats', statsMock.withBtcLocked);
    component = await render(<Stats/>);
  });

  test('+@mobile +@dark-mode', async() => {
    await expect(component).toHaveScreenshot();
  });
});

test('no gas info', async({ render, mockApiResponse, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_STATS_API_HOST', '' ],
  ]);
  await mockApiResponse('general:stats', statsMock.withoutGasInfo);
  const component = await render(<Stats/>);

  await expect(component).toHaveScreenshot();
});

test('4 items default view +@mobile -@default', async({ render, mockApiResponse, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_HOMEPAGE_STATS', '["total_txs","gas_tracker","wallet_addresses","total_blocks"]' ],
    [ 'NEXT_PUBLIC_STATS_API_HOST', '' ],
  ]);
  await mockApiResponse('general:stats', statsMock.base);
  const component = await render(<Stats/>);
  await expect(component).toHaveScreenshot();
});

test('3 items default view +@mobile -@default', async({ render, mockApiResponse, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_HOMEPAGE_STATS', '["total_txs","wallet_addresses","total_blocks"]' ],
    [ 'NEXT_PUBLIC_STATS_API_HOST', '' ],
  ]);
  await mockApiResponse('general:stats', statsMock.base);
  const component = await render(<Stats/>);
  await expect(component).toHaveScreenshot();
});
