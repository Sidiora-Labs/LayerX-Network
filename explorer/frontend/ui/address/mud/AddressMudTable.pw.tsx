import { Box } from '@chakra-ui/react';
import React from 'react';

import { mudRecords } from 'mocks/mud/mudTables';
import { test, expect } from 'playwright/lib';

import AddressMudTable from './AddressMudTable';

const ADDRESS_HASH = 'hash';
const TABLE_ID = '123';
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:mud_records', mudRecords, { pathParams: { hash: ADDRESS_HASH, table_id: TABLE_ID } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudTable tableId={ TABLE_ID }/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});

test('expanded view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:mud_records', mudRecords, { pathParams: { hash: ADDRESS_HASH, table_id: TABLE_ID } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudTable tableId={ TABLE_ID }/>
    </Box>,
    { hooksConfig },
  );

  await component.getByLabel('show/hide columns').click();

  await expect(component).toHaveScreenshot();
});

test('empty +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse(
    'general:mud_records',
    { ...mudRecords, items: [] },
    { pathParams: { hash: ADDRESS_HASH, table_id: TABLE_ID } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudTable tableId={ TABLE_ID }/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});
