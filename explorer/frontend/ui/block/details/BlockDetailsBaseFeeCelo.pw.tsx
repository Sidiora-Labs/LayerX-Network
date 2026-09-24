import { Grid } from '@chakra-ui/react';
import React from 'react';

import type { BlockBaseFeeCelo } from 'types/api/block';

import * as blockMock from 'mocks/blocks/block';
import { test, expect } from 'playwright/lib';

import BlockDetailsBaseFeeCelo from './BlockDetailsBaseFeeCelo';

test('base view +@mobile', async({ render }) => {
  const component = await render(
    <Grid
      columnGap={ 8 }
      rowGap={{ base: 3, lg: 3 }}
      templateColumns={{ base: 'minmax(0, 1fr)', lg: 'minmax(min-content, 200px) minmax(0, 1fr)' }}
      overflow="hidden"
    >
      <BlockDetailsBaseFeeCelo data={ blockMock.celo.celo?.base_fee as BlockBaseFeeCelo }/>
    </Grid>,
  );
  await expect(component).toHaveScreenshot();
});
