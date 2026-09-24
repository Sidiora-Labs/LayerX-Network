import React from 'react';

import * as validatorsMock from 'mocks/validators/zilliqa';
import { test, expect } from 'playwright/lib';

import Validators from './ValidatorsZilliqa';

const chainType = 'zilliqa';

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VALIDATORS_CHAIN_TYPE', chainType ],
  ]);
  await mockApiResponse('general:validators_zilliqa', validatorsMock.validatorsResponse);
  await mockTextAd();

  const component = await render(<Validators/>);

  await expect(component).toHaveScreenshot();
});
