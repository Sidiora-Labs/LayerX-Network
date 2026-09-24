import { Box } from '@chakra-ui/react';
import React from 'react';

import { epochRewards } from 'mocks/address/epochRewards';
import { test, expect } from 'playwright/lib';

import AddressEpochRewards from './AddressEpochRewards';

const ADDRESS_HASH = '0x1234';
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:address_epoch_rewards', epochRewards, { pathParams: { hash: ADDRESS_HASH } });
  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressEpochRewards/>
    </Box>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot();
});
