import { Box } from '@chakra-ui/react';
import React from 'react';

import { ZKEVM_L2_TX_STATUSES } from 'types/api/transaction';

import { test, expect } from 'playwright/lib';

import VerificationSteps from './VerificationSteps';

test('first step +@mobile +@dark-mode', async({ render }) => {
  const component = await render(
    <Box p={ 10 }>
      <VerificationSteps currentStep={ ZKEVM_L2_TX_STATUSES[0] } steps={ ZKEVM_L2_TX_STATUSES }/>
    </Box>,
  );
  await expect(component).toHaveScreenshot();
});

test('second status', async({ render }) => {
  const component = await render(
    <VerificationSteps currentStep={ ZKEVM_L2_TX_STATUSES[1] } steps={ ZKEVM_L2_TX_STATUSES }/>,
  );
  await expect(component).toHaveScreenshot();
});
