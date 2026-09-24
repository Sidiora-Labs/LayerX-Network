import { Box } from '@chakra-ui/react';
import type { Locator } from '@playwright/test';
import React from 'react';

import * as tokensMock from 'mocks/address/tokens';
import * as addressMock from 'mocks/multichain/address';
import * as chainDataMock from 'mocks/multichain/chains';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import MultichainAddressPortfolio from './MultichainAddressPortfolio';

const CURRENT_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

test.beforeEach(async({ mockMultichainConfig, mockEnvs }) => {
  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
});

test.describe('nfts', () => {
  const hooksConfig = {
    router: {
      query: { hash: CURRENT_ADDRESS, tab: 'portfolio_nfts' },
    },
  };
  let component: Locator;

  test.beforeEach(async({ render, mockApiResponse, mockAssetResponse }) => {
    await mockApiResponse(
      'general:address_collections',
      { ...tokensMock.collections, items: [ tokensMock.collections.items[2] ] },
      { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { type: [] }, chainConfig: chainDataMock.chainA },
    );
    await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/image_s.jpg');

    component = await render(
      <Box pt={{ base: 0, lg: '30px' }}>
        <MultichainAddressPortfolio addressData={ addressMock.addressA } isLoading={ false }/>
      </Box>,
      { hooksConfig },
    );
  });

  test('base view', async() => {
    await expect(component).toHaveScreenshot();
  });

  test.describe('mobile', () => {
    test.use({ viewport: pwConfig.viewport.mobile });

    test('base view', async() => {
      await expect(component).toHaveScreenshot();
    });
  });
});
