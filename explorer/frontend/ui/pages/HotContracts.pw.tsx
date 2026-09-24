import React from 'react';

import * as contractsMock from 'mocks/contracts/index';
import * as statsMock from 'mocks/stats/index';
import { test, expect } from 'playwright/lib';
import { getIntervalValueFromQuery } from 'ui/hotContracts/utils';

import HotContracts from './HotContracts';

test('base view +@mobile', async({ render, mockTextAd, mockApiResponse, mockEnvs }) => {
  test.slow();
  await mockEnvs([ [ 'NEXT_PUBLIC_VIEWS_TOKEN_SCAM_TOGGLE_ENABLED', 'true' ] ]);
  await mockTextAd();
  await mockApiResponse(
    'general:stats_hot_contracts',
    contractsMock.hotContractsResponse,
    { queryParams: { scale: getIntervalValueFromQuery(undefined) } },
  );
  await mockApiResponse('general:stats', { ...statsMock.base, coin_price: '3214.42' });

  const component = await render(<HotContracts/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
