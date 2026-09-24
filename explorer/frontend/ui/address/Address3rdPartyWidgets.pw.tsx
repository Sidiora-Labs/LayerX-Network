import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as widgetsMock from 'mocks/address/widgets';
import type { TestFnArgs } from 'playwright/lib';
import { test, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Address3rdPartyWidgets from './Address3rdPartyWidgets';

const WIDGETS_CONFIG_URL = 'http://localhost:4000/address-3rd-party-widgets-config.json';
const ADDRESS_HASH = addressMock.hash;
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

const testFn = (isMobile: boolean) => async({ render, mockConfigResponse, mockAssetResponse, mockEnvs, mockApiResponse, page }: TestFnArgs) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS', JSON.stringify(widgetsMock.widgets) ],
    [ 'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL', WIDGETS_CONFIG_URL ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL', WIDGETS_CONFIG_URL, widgetsMock.config);

  await Promise.all(widgetsMock.widgets.map((widget, i) =>
    mockApiResponse(
      'general:address_3rd_party_info',
      { value: widgetsMock.values[i] },
      { pathParams: { name: widget }, queryParams: { address: ADDRESS_HASH, chain_id: '1' } },
    ),
  ));
  await mockAssetResponse(widgetsMock.config[widgetsMock.widgets[0]].icon, './playwright/mocks/image_s.jpg');

  const component = await render(<Address3rdPartyWidgets showAll addressType="contract"/>, { hooksConfig });

  if (!isMobile) {
    await page.getByText(widgetsMock.config[widgetsMock.widgets[0]].name).hover({ force: true }); // eslint-disable-line playwright/no-force-option
  }

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
};

test('base view +@dark-mode', testFn(false));

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', testFn(true));
});
