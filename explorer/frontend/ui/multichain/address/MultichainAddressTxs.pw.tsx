import { Box } from '@chakra-ui/react';
import type { Locator } from '@playwright/test';
import React from 'react';

import * as countersMock from 'mocks/address/counters';
import * as addressMock from 'mocks/multichain/address';
import * as chainDataMock from 'mocks/multichain/chains';
import * as txMock from 'mocks/txs/tx';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import MultichainAddressTxs from './MultichainAddressTxs';

const CURRENT_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

const DEFAULT_PAGINATION = { block_number: 1, index: 1, items_count: 1 };

test.describe('local txs', () => {
  const hooksConfig = {
    router: {
      query: { hash: CURRENT_ADDRESS, tab: 'txs_local' },
    },
  };
  let component: Locator;

  test.beforeEach(async({ render, mockMultichainConfig, mockEnvs, mockApiResponse, mockAssetResponse, createSocket }) => {
    await mockMultichainConfig();
    await mockEnvs(ENVS_MAP.multichain);
    await mockApiResponse(
      'general:address_txs',
      { items: [ txMock.base ], next_page_params: DEFAULT_PAGINATION },
      { pathParams: { hash: CURRENT_ADDRESS }, chainConfig: chainDataMock.chainA },
    );
    await mockApiResponse(
      'general:address_counters',
      countersMock.forContract,
      { pathParams: { hash: CURRENT_ADDRESS }, chainConfig: chainDataMock.chainA },
    );
    await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/image_s.jpg');

    component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <MultichainAddressTxs addressData={ addressMock.addressA } isLoading={ false }/>
      </Box>,
      { hooksConfig },
      { withSocket: true },
    );
    await createSocket();
  });

  test('base view', async() => {
    await expect(component).toHaveScreenshot({ maxDiffPixels: 5 });
  });

  test.describe('mobile', () => {
    test.use({ viewport: pwConfig.viewport.mobile });

    test('base view', async() => {
      await expect(component).toHaveScreenshot({ maxDiffPixels: 5 });
    });
  });
});
