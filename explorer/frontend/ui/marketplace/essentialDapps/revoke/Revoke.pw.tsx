import React from 'react';

import * as chainDataMock from 'mocks/multichain/chains';
import { test, expect } from 'playwright/lib';

import Revoke from './Revoke';

const ESSENTIAL_DAPPS_CONFIG = JSON.stringify({
  revoke: { chains: [ chainDataMock.chainA.id ] },
});

test('base view +@dark-mode +@mobile', async({ render, mockEnvs, mockEssentialDappsChainsConfig, mockAssetResponse }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MARKETPLACE_ENABLED', 'true' ],
    [ 'NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG', ESSENTIAL_DAPPS_CONFIG ],
  ]);
  await mockEssentialDappsChainsConfig();
  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/image_s.jpg');

  const component = await render(<Revoke/>);

  await expect(component).toHaveScreenshot();
});
