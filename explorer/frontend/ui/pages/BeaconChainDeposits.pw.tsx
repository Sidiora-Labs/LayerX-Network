import React from 'react';

import { data as depositsData } from 'mocks/deposits/deposits';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import BeaconChainDeposits from './BeaconChainDeposits';

test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  await mockEnvs(ENVS_MAP.beaconChain);
  await mockTextAd();
  await mockApiResponse('general:deposits', depositsData);
  await mockApiResponse('general:deposits_counters', { deposits_count: '111111' });
  const component = await render(<BeaconChainDeposits/>);
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
    await mockEnvs(ENVS_MAP.beaconChain);
    await mockTextAd();
    await mockApiResponse('general:deposits', depositsData);
    await mockApiResponse('general:deposits_counters', { deposits_count: '111111' });
    const component = await render(<BeaconChainDeposits/>);
    await expect(component).toHaveScreenshot();
  });
});
