import React from 'react';

import * as validatorsMock from 'mocks/validators/stability';
import { test, expect } from 'playwright/lib';

import Validators from './ValidatorsStability';

const chainType = 'stability';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VALIDATORS_CHAIN_TYPE', chainType ],
  ]);
  await mockApiResponse('general:validators_stability', validatorsMock.validatorsResponse);
  await mockApiResponse('general:validators_stability_counters', validatorsMock.validatorsCountersResponse);
  await mockTextAd();

  const component = await render(<Validators/>);

  await expect(component).toHaveScreenshot();
});
