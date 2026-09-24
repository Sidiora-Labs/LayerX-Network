import React from 'react';

import config from 'configs/app';
import * as addressMock from 'mocks/address/address';
import * as poolMock from 'mocks/pools/pool';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Pool from './Pool';

const addressHash = '0x1234';
const hooksConfig = {
  router: {
    query: { hash: addressHash },
  },
};

test('base view +@mobile +@dark-mode', async({ render, mockApiResponse, mockTextAd, mockAssetResponse, page }) => {
  await mockTextAd();
  await mockApiResponse('contractInfo:pool', poolMock.base, { pathParams: { chainId: config.chain.id, hash: addressHash } });
  await mockApiResponse('general:address', addressMock.contract, { pathParams: { hash: poolMock.base.pool_id } });
  await mockAssetResponse(poolMock.base.quote_token_icon_url as string, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(poolMock.base.base_token_icon_url as string, './playwright/mocks/image_md.jpg');
  const component = await render(<Pool/>, { hooksConfig });
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
    maxDiffPixels: 20,
  });
});
