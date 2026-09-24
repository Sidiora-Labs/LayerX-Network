import React from 'react';

import { test, expect } from 'playwright/lib';

import AssetValue from './AssetValue';

test.use({ viewport: { width: 300, height: 100 } });

test('no exchange rate', async({ render }) => {
  const component = await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('current exchange rate, horizontal layout', async({ render }) => {
  const component = await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
      exchangeRate="1234.56"
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('current exchange rate, vertical layout', async({ render }) => {
  const component = await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
      exchangeRate="1234.56"
      layout="vertical"
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('current + historic, default shows historic with tag', async({ render }) => {
  const component = await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
      exchangeRate="1234.56"
      historicalExchangeRate="1100.00"
      hasExchangeRateToggle
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('current + historic, tooltip shows historic label', async({ render, page }) => {
  await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
      exchangeRate="1234.56"
      historicalExchangeRate="1100.00"
      hasExchangeRateToggle
    />,
  );

  // hover over the USD value to trigger the tooltip
  await page.locator('text=$').first().hover();
  await expect(page.getByText('Estimated value on day of txn')).toBeVisible();
  await expect(page).toHaveScreenshot();
});

test('current + historic, toggle to current', async({ render, page }) => {
  await render(
    <AssetValue
      amount="12345678901234567890"
      decimals="18"
      exchangeRate="1234.56"
      historicalExchangeRate="1100.00"
      hasExchangeRateToggle
    />,
  );

  // click the tag to toggle to current
  await page.locator('text=$').first().click();

  // verify tooltip shows current label
  await page.locator('text=$').first().hover();
  await expect(page.getByText('Current value')).toBeVisible();
  await expect(page).toHaveScreenshot();
});

test('zero value, no tag shown', async({ render }) => {
  const component = await render(
    <AssetValue
      amount="0"
      decimals="18"
      exchangeRate="1234.56"
      historicalExchangeRate="1100.00"
      hasExchangeRateToggle
    />,
  );
  await expect(component).toHaveScreenshot();
});
