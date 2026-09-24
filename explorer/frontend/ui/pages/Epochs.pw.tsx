import React from 'react';

import { list as epochsList } from 'mocks/epochs/celo';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import Epochs from './Epochs';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  await mockEnvs(ENVS_MAP.celo);
  await mockTextAd();
  await mockApiResponse('general:epochs_celo', epochsList);

  const component = await render(<Epochs/>);

  await expect(component).toHaveScreenshot();
});
