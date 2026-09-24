import React from 'react';

import * as validatorsMock from 'mocks/validators/zilliqa';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import ValidatorDetails from './ValidatorDetails';

test('base view +@mobile', async({ render, page }) => {
  const component = await render(<ValidatorDetails data={ validatorsMock.validatorDetails } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
