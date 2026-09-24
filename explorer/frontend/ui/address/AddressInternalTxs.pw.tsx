import { Box } from '@chakra-ui/react';
import React from 'react';

import * as internalTxsMock from 'mocks/txs/internalTxs';
import { test, expect } from 'playwright/lib';

import AddressInternalTxs from './AddressInternalTxs';

const ADDRESS_HASH = internalTxsMock.base.from.hash;
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  test.slow();
  await mockApiResponse('general:address_internal_txs', internalTxsMock.baseResponse, { pathParams: { hash: ADDRESS_HASH } });
  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressInternalTxs/>
    </Box>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
