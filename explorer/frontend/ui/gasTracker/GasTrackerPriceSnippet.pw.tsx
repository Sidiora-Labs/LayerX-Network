import React from 'react';

import type { GasPriceInfo } from 'types/api/stats';

import * as statsMock from 'mocks/stats/index';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import GasTrackerPriceSnippet from './GasTrackerPriceSnippet';

test.use({ viewport: pwConfig.viewport.md });

const data = statsMock.base.gas_prices?.fast as GasPriceInfo;
const clip = { x: 0, y: 0, width: 334, height: 204 };

test('with usd as primary unit +@dark-mode', async({ render, page }) => {
  await render(
    <GasTrackerPriceSnippet
      data={ data }
      type="fast"
      isLoading={ false }
    />,
  );
  await expect(page).toHaveScreenshot({ clip });
});

test('loading state', async({ render, page }) => {
  await render(
    <GasTrackerPriceSnippet
      data={ data }
      type="fast"
      isLoading={ true }
    />,
  );
  await expect(page).toHaveScreenshot({ clip });
});

test('with gwei as primary unit +@dark-mode', async({ render, page, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_GAS_TRACKER_UNITS', '["gwei","usd"]' ],
  ]);
  await render(
    <GasTrackerPriceSnippet
      data={ data }
      type="slow"
      isLoading={ false }
    />,
  );
  await expect(page).toHaveScreenshot({ clip });
});

test('with zero values', async({ render, page }) => {
  const data = {
    fiat_price: '1.74',
    price: 0.0,
    time: 0,
    base_fee: 0,
    priority_fee: 0,
  };

  await render(
    <GasTrackerPriceSnippet
      data={ data }
      type="slow"
      isLoading={ false }
    />,
  );
  await expect(page).toHaveScreenshot({ clip });
});

test('with small values', async({ render, page }) => {
  const data = {
    fiat_price: '0.0042',
    price: 0.042,
    time: 0,
    base_fee: 0,
    priority_fee: 0,
  };

  await render(
    <GasTrackerPriceSnippet
      data={ data }
      type="slow"
      isLoading={ false }
    />,
  );
  await expect(page).toHaveScreenshot({ clip });
});
