import React from 'react';

import * as blobsMock from 'mocks/blobs/blobs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Blob from './Blob';

const hooksConfig = {
  router: {
    query: { hash: blobsMock.base1.hash },
  },
};

test.beforeEach(async({ mockTextAd }) => {
  await mockTextAd();
});

test('base view +@mobile +@dark-mode', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('general:blob', blobsMock.base1, { pathParams: { hash: blobsMock.base1.hash } });
  const component = await render(<Blob/>, { hooksConfig });
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('without data', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('general:blob', blobsMock.withoutData, { pathParams: { hash: blobsMock.base1.hash } });
  const component = await render(<Blob/>, { hooksConfig });
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
