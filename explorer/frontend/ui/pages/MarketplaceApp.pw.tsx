import { Flex } from '@chakra-ui/react';
import React from 'react';
import { numberToHex } from 'viem';

import config from 'configs/app';
import { apps as appsMock } from 'mocks/apps/apps';
import type { TestFnArgs } from 'playwright/lib';
import { test, expect, devices } from 'playwright/lib';

import MarketplaceApp from './MarketplaceApp';

const hooksConfig = {
  router: {
    query: { id: appsMock[0].id },
    isReady: true,
  },
};

const testFn = async({ render, mockAssetResponse, mockEnvs, mockRpcResponse, mockApiResponse }: TestFnArgs) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_ENABLED', 'true' ],
  ]);
  await mockApiResponse('admin:marketplace_dapp', appsMock[0], { pathParams: { chainId: config.chain.id, dappId: appsMock[0].id } });
  await mockAssetResponse(appsMock[0].url, './mocks/apps/app.html');
  await mockRpcResponse([ {
    Method: 'eth_chainId',
    ReturnType: numberToHex(Number(config.chain.id)),
  } ]);

  const component = await render(
    <Flex flexDirection="column" mx={{ base: 4, lg: 6 }} h="100vh">
      <MarketplaceApp/>
    </Flex>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
};

test('base view +@dark-mode', testFn);

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', testFn);
});
