import React from 'react';

import type { GasPriceInfo } from 'types/api/stats';

import * as statsMock from 'mocks/stats/index';
import { test, expect } from 'playwright/lib';

import GasInfoTooltip from './GasInfoTooltip';
import GasPrice from './GasPrice';

const dataUpdatedAt = 0;

test.use({ viewport: { width: 300, height: 300 } });

test('all data', async({ render, page }) => {
  await render(
    <GasInfoTooltip data={ statsMock.base } dataUpdatedAt={ dataUpdatedAt }>
      <span>Gas <GasPrice data={ statsMock.base.gas_prices?.average as GasPriceInfo }/></span>
    </GasInfoTooltip>,
  );

  await page.getByText(/gas/i).hover();
  await page.getByText(/last update/i).isVisible();
  await expect(page).toHaveScreenshot();
});

test('without primary unit price', async({ render, page }) => {
  await render(
    <GasInfoTooltip data={ statsMock.withoutFiatPrices } dataUpdatedAt={ dataUpdatedAt }>
      <span>Gas: <GasPrice data={ statsMock.withoutFiatPrices.gas_prices?.average as GasPriceInfo }/></span>
    </GasInfoTooltip>,
  );

  await page.getByText(/gas/i).hover();
  await page.getByText(/last update/i).isVisible();
  await expect(page).toHaveScreenshot();
});

test('without secondary unit price', async({ render, page }) => {
  await render(
    <GasInfoTooltip data={ statsMock.withoutGweiPrices } dataUpdatedAt={ dataUpdatedAt }>
      <span>Gas: <GasPrice data={ statsMock.withoutGweiPrices.gas_prices?.average as GasPriceInfo }/></span>
    </GasInfoTooltip>,
  );

  await page.getByText(/gas/i).hover();
  await page.getByText(/last update/i).isVisible();
  await expect(page).toHaveScreenshot();
});

test('no data', async({ render, page }) => {
  await render(
    <GasInfoTooltip data={ statsMock.withoutBothPrices } dataUpdatedAt={ dataUpdatedAt }>
      <span>Gas: <GasPrice data={ statsMock.withoutBothPrices.gas_prices?.average as GasPriceInfo }/></span>
    </GasInfoTooltip>,
  );

  await page.getByText(/gas/i).hover();
  await page.getByText(/last update/i).isVisible();
  await expect(page).toHaveScreenshot();
});

test.describe('one unit', () => {
  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_GAS_TRACKER_UNITS', '["gwei"]' ],
    ]);
  });

  test('with data', async({ render, page }) => {
    await render(
      <GasInfoTooltip data={ statsMock.withoutFiatPrices } dataUpdatedAt={ dataUpdatedAt }>
        <span>Gas: <GasPrice data={ statsMock.withoutFiatPrices.gas_prices?.average as GasPriceInfo }/></span>
      </GasInfoTooltip>,
    );

    await page.getByText(/gas/i).hover();
    await page.getByText(/last update/i).isVisible();
    await expect(page).toHaveScreenshot();
  });

  test('without data', async({ render, page }) => {
    await render(
      <GasInfoTooltip data={ statsMock.withoutGweiPrices } dataUpdatedAt={ dataUpdatedAt }>
        <span>Gas: <GasPrice data={ statsMock.withoutGweiPrices.gas_prices?.average as GasPriceInfo }/></span>
      </GasInfoTooltip>,
    );

    await page.getByText(/gas/i).hover();
    await page.getByText(/last update/i).isVisible();
    await expect(page).toHaveScreenshot();
  });
});
