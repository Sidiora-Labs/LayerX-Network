import React from 'react';

import { test, expect } from 'playwright/lib';

import BlobDataType from './BlobDataType';

test.use({ viewport: { width: 100, height: 50 } });

test('image data', async({ render }) => {
  const component = await render(<BlobDataType data="0x89504E470D0A1A0A0000000D494844520000003C0000003C0403"/>);
  await expect(component).toHaveScreenshot();
});

test('raw data', async({ render }) => {
  const component = await render(<BlobDataType data="0x010203040506"/>);
  await expect(component).toHaveScreenshot();
});

test('text data', async({ render }) => {
  const component = await render(<BlobDataType data="0x7b226e616d65223a22706963732f"/>);
  await expect(component).toHaveScreenshot();
});
