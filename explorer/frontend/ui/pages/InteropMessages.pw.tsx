import React from 'react';

import * as interopMessageMock from 'mocks/interop/interop';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import InteropMessages from './InteropMessages';

const MESSAGES_RESPONSE = {
  items: [
    interopMessageMock.interopMessageIn,
    interopMessageMock.interopMessageIn1,
    interopMessageMock.interopMessageOut,
    interopMessageMock.interopMessageOut1,
  ].map((item, index) => ({ ...item, init_transaction_hash: `${ item.init_transaction_hash.slice(0, -1) }${ index }` })),
  next_page_params: {
    init_transaction_hash: '1',
    items_count: 4,
    timestamp: 1719456000,
  },
};

test('default view', async({ render, mockTextAd, mockAssetResponse, mockApiResponse }) => {
  await mockTextAd();
  await mockAssetResponse(interopMessageMock.chain.chain_logo as string, './playwright/mocks/image_s.jpg');
  await mockApiResponse('general:optimistic_l2_interop_messages', MESSAGES_RESPONSE);
  await mockApiResponse('general:optimistic_l2_interop_messages_count', 4000000);
  const component = await render(<InteropMessages/>);
  await expect(component).toHaveScreenshot({ maxDiffPixels: 30 });
});

test.describe('mobile', () => {
  test.use({ viewport: pwConfig.viewport.mobile });
  test('default view', async({ render, mockTextAd, mockAssetResponse, mockApiResponse }) => {
    await mockTextAd();
    await mockAssetResponse(interopMessageMock.chain.chain_logo as string, './playwright/mocks/image_s.jpg');
    await mockApiResponse('general:optimistic_l2_interop_messages', MESSAGES_RESPONSE);
    await mockApiResponse('general:optimistic_l2_interop_messages_count', 4000000);
    const component = await render(<InteropMessages/>);
    await expect(component).toHaveScreenshot({ maxDiffPixels: 30 });
  });
});
