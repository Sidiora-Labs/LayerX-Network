import { Box } from '@chakra-ui/react';
import React from 'react';

import { indexingStatus } from 'mocks/stats/index';
import { test, expect } from 'playwright/lib';

import LayoutHome from './LayoutHome';

// FIXME: at the moment, in the docker container playwright make screenshot before the page is completely loaded
// I cannot figure out the reason, so I skip this test for now
test.skip('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:homepage_indexing_status', indexingStatus);
  const component = await render(
    <LayoutHome>
      <Box pt={ 10 }>Error</Box>
    </LayoutHome>,
  );
  await expect(component).toHaveScreenshot();
});
