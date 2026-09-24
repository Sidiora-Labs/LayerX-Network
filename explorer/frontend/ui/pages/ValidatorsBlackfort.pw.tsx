import React from 'react';

import * as validatorsMock from 'mocks/validators/blackfort';
import { test, expect } from 'playwright/lib';

import Validators from './ValidatorsBlackfort';

const chainType = 'blackfort';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VALIDATORS_CHAIN_TYPE', chainType ],
  ]);
  await mockApiResponse('general:validators_blackfort', validatorsMock.validatorsResponse);
  await mockApiResponse('general:validators_blackfort_counters', validatorsMock.validatorsCountersResponse);
  await mockTextAd();

  const component = await render(<Validators/>);

  await expect(component).toHaveScreenshot();
});
