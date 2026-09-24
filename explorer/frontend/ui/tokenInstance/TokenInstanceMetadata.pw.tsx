import React from 'react';

import * as tokenInstanceMock from 'mocks/tokens/tokenInstance';
import { test, expect } from 'playwright/lib';

import TokenInstanceMetadata from './TokenInstanceMetadata';

test('base view +@mobile', async({ render }) => {
  const component = await render(<TokenInstanceMetadata data={ tokenInstanceMock.withRichMetadata.metadata }/>);

  await component.getByRole('button', { name: /png/i }).click();
  await component.getByRole('button', { name: /primary/i }).click();

  await component.getByRole('button', { name: /secondary/i }).click();
  await component.getByRole('button', { name: /more/i }).click();

  await component.getByRole('button', { name: /webp/i }).click();
  await component.getByRole('button', { name: /attributes/i }).click();
  await component.getByRole('button', { name: /tags/i }).click();

  await expect(component).toHaveScreenshot();
});

test('raw view', async({ render, page }) => {
  const component = await render(<TokenInstanceMetadata data={ tokenInstanceMock.withRichMetadata.metadata }/>);
  await component.getByRole('combobox').click();
  await page.getByRole('option', { name: 'JSON' }).click();
  await expect(component).toHaveScreenshot();
});
