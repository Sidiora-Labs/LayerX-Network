import React from 'react';

import * as epochMock from 'mocks/epochs/celo';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import Epoch from './Epoch';

test('base view +@mobile', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  const hooksConfig = {
    router: {
      query: { number: String(epochMock.epoch1.number) },
    },
  };

  await mockEnvs(ENVS_MAP.celo);
  await mockTextAd();
  await mockApiResponse('general:epoch_celo', epochMock.epoch1, { pathParams: { number: String(epochMock.epoch1.number) } });

  const component = await render(<Epoch/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test('unfinalized epoch', async({ render, mockEnvs, mockTextAd, mockApiResponse }) => {
  const hooksConfig = {
    router: {
      query: { number: String(epochMock.epochUnfinalized.number) },
    },
  };

  await mockEnvs(ENVS_MAP.celo);
  await mockTextAd();
  await mockApiResponse('general:epoch_celo', epochMock.epochUnfinalized, { pathParams: { number: String(epochMock.epochUnfinalized.number) } });

  const component = await render(<Epoch/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});
