import { Box } from '@chakra-ui/react';
import React from 'react';

import { mudRecord } from 'mocks/mud/mudTables';
import { test, expect, devices } from 'playwright/lib';

import AddressMudRecord from './AddressMudRecord';

const ADDRESS_HASH = 'hash';
const TABLE_ID = '123';
const RECORD_ID = '234';
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

test('base view', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:mud_record', mudRecord, { pathParams: { hash: ADDRESS_HASH, table_id: TABLE_ID, record_id: RECORD_ID } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudRecord tableId={ TABLE_ID } recordId={ RECORD_ID }/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:mud_record', mudRecord, { pathParams: { hash: ADDRESS_HASH, table_id: TABLE_ID, record_id: RECORD_ID } });

    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressMudRecord tableId={ TABLE_ID } recordId={ RECORD_ID }/>
      </Box>,
      { hooksConfig },
    );

    await expect(component).toHaveScreenshot();
  });
});
