import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as countersMock from 'mocks/address/counters';
import * as tokensMock from 'mocks/address/tokens';
import * as widgetsMock from 'mocks/address/widgets';
import type { TestFnArgs } from 'playwright/lib';
import { test, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import AddressDetails from './AddressDetails';
import MockAddressPage from './testUtils/MockAddressPage';
import type { AddressCountersQuery } from './utils/useAddressCountersQuery';
import type { AddressQuery } from './utils/useAddressQuery';

const WIDGETS_CONFIG_URL = 'http://localhost:4000/address-3rd-party-widgets-config.json';
const ADDRESS_HASH = addressMock.hash;
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

const testWidgetsFn = (isMobile: boolean) => async({ render, mockConfigResponse, mockAssetResponse, mockEnvs, mockApiResponse, page }: TestFnArgs) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS', JSON.stringify(widgetsMock.widgets) ],
    [ 'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL', WIDGETS_CONFIG_URL ],
  ]);
  await mockConfigResponse('NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL', WIDGETS_CONFIG_URL, widgetsMock.config);

  await mockApiResponse('general:address', addressMock.contract, { pathParams: { hash: ADDRESS_HASH } });
  await mockApiResponse('general:address_counters', countersMock.forContract, { pathParams: { hash: ADDRESS_HASH } });
  await Promise.all(widgetsMock.widgets.map((widget, i) =>
    mockApiResponse(
      'general:address_3rd_party_info',
      { value: widgetsMock.values[i] },
      { pathParams: { name: widget }, queryParams: { address: ADDRESS_HASH, chain_id: '1' } },
    ),
  ));
  await mockAssetResponse(widgetsMock.config[widgetsMock.widgets[0]].icon, './playwright/mocks/image_s.jpg');

  const component = await render(
    <AddressDetails
      addressQuery={{ data: addressMock.contract } as AddressQuery}
      countersQuery={{ data: countersMock.forContract } as AddressCountersQuery}
    />,
    { hooksConfig },
  );

  if (!isMobile) {
    await page.getByText(widgetsMock.config[widgetsMock.widgets[0]].name).hover({ force: true }); // eslint-disable-line playwright/no-force-option
  }

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
};

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('contract', async({ render, mockApiResponse, page }) => {
    await mockApiResponse('general:address', addressMock.contract, { pathParams: { hash: ADDRESS_HASH } });

    const component = await render(
      <AddressDetails
        addressQuery={{ data: addressMock.contract } as AddressQuery}
        countersQuery={{ data: countersMock.forContract } as AddressCountersQuery}
      />,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

  test('validator', async({ render, page, mockApiResponse }) => {
    await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: ADDRESS_HASH } });

    const component = await render(
      <AddressDetails
        addressQuery={{ data: addressMock.validator } as AddressQuery}
        countersQuery={{ data: countersMock.forValidator } as AddressCountersQuery}
      />,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

  test('filecoin', async({ render, mockApiResponse, page }) => {
    await mockApiResponse('general:address', addressMock.filecoin, { pathParams: { hash: ADDRESS_HASH } });

    const component = await render(
      <AddressDetails
        addressQuery={{ data: addressMock.filecoin } as AddressQuery}
        countersQuery={{ data: countersMock.forValidator } as AddressCountersQuery}
      />,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

  test('with widgets', testWidgetsFn(true));
});

test('contract', async({ render, page, mockApiResponse }) => {
  await mockApiResponse('general:address', addressMock.contract, { pathParams: { hash: ADDRESS_HASH } });

  const component = await render(
    <AddressDetails
      addressQuery={{ data: addressMock.contract } as AddressQuery}
      countersQuery={{ data: countersMock.forContract } as AddressCountersQuery}
    />,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

// there's an unexpected timeout occurred in this test
test.fixme('token', async({ render, mockApiResponse, injectMetaMaskProvider, page }) => {
  await mockApiResponse('general:address', addressMock.token, { pathParams: { hash: ADDRESS_HASH } });
  await mockApiResponse('general:address_tokens', tokensMock.erc20List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-20' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc721List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-721' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc1155List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-1155' }, times: 1 });
  await mockApiResponse('general:address_tokens', tokensMock.erc404List, { pathParams: { hash: ADDRESS_HASH }, queryParams: { type: 'ERC-404' }, times: 1 });
  await injectMetaMaskProvider();

  const component = await render(
    <MockAddressPage>
      <AddressDetails addressQuery={{ data: addressMock.token } as AddressQuery} countersQuery={{ data: countersMock.forToken } as AddressCountersQuery}/>
    </MockAddressPage>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('validator', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('general:address', addressMock.validator, { pathParams: { hash: ADDRESS_HASH } });

  const component = await render(
    <AddressDetails
      addressQuery={{ data: addressMock.validator } as AddressQuery}
      countersQuery={{ data: countersMock.forValidator } as AddressCountersQuery}
    />,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('filecoin', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('general:address', addressMock.filecoin, { pathParams: { hash: ADDRESS_HASH } });

  const component = await render(
    <AddressDetails
      addressQuery={{ data: addressMock.filecoin } as AddressQuery}
      countersQuery={{ data: countersMock.forValidator } as AddressCountersQuery}
    />,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with widgets', testWidgetsFn(false));
