import React from 'react';

import { test, expect } from 'playwright/lib';

import DefaultView from './specs/DefaultView';
import LongNameAndManyTags from './specs/LongNameAndManyTags';
import WithTextAd from './specs/WithTextAd';

test.beforeEach(async({ mockTextAd, mockAssetResponse }) => {
  await mockTextAd();
  await mockAssetResponse('https://example.com/logo.png', './playwright/mocks/image_s.jpg');
});

test('default view +@mobile', async({ render, page }) => {
  const component = await render(<DefaultView/>);
  await page.mouse.move(1000, 1000);
  await expect(component).toHaveScreenshot();
});

test('with text ad +@mobile', async({ render }) => {
  const component = await render(<WithTextAd/>);
  await expect(component).toHaveScreenshot();
});

test('with long name and many tags +@mobile', async({ render }) => {
  const component = await render(<LongNameAndManyTags/>);
  await expect(component).toHaveScreenshot();
});
