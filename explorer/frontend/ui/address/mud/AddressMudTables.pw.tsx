import { Box } from '@chakra-ui/react';
import React from 'react';

import { mudTables } from 'mocks/mud/mudTables';
import { test, expect } from 'playwright/lib';

import AddressMudTables from './AddressMudTables';

const ADDRESS_HASH = 'hash';
const hooksConfig = {
  router: {
    query: { hash: ADDRESS_HASH, q: 'o' },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:mud_tables', mudTables, { pathParams: { hash: ADDRESS_HASH }, queryParams: { q: 'o' } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudTables/>
    </Box>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot();
});

test('with schema opened +@mobile', async({ render, mockApiResponse }, testInfo) => {
  await mockApiResponse('general:mud_tables', mudTables, { pathParams: { hash: ADDRESS_HASH }, queryParams: { q: 'o' } });

  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressMudTables/>
    </Box>,
    { hooksConfig },
  );

  await component.getByLabel('View schema').nth(testInfo.project.name === 'mobile' ? 2 : 0).click();

  await expect(component).toHaveScreenshot();
});
