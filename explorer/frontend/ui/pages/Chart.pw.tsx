import React from 'react';

import * as statsLineMock from 'mocks/stats/line';
import { test, expect } from 'playwright/lib';
import { formatDate } from 'ui/shared/chart/utils';

import Chart from './Chart';

const CHART_ID = 'averageGasPrice';

test.beforeEach(async({ mockTextAd }) => {
  await mockTextAd();
});

const hooksConfig = {
  router: {
    query: { id: CHART_ID },
  },
};

test('base view +@dark-mode +@mobile', async({ render, mockApiResponse, page }) => {
  const date = new Date();
  date.setMonth(date.getMonth() - 1);

  const chartApiUrl = await mockApiResponse(
    'stats:line',
    statsLineMock.averageGasPrice,
    {
      pathParams: { id: CHART_ID },
      queryParams: {
        from: formatDate(date),
        to: '2022-11-11',
        resolution: 'DAY',
      },
    },
  );

  const component = await render(<Chart/>, { hooksConfig });
  await page.waitForResponse(chartApiUrl);
  await page.waitForFunction(() => {
    return document.querySelector('path[data-name="chart-fullscreen"]')?.getAttribute('opacity') === '1';
  });
  await expect(component).toHaveScreenshot();
});
