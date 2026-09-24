import React from 'react';

import config from 'configs/app';
import { test, expect } from 'playwright/lib';
import { port as socketPort } from 'playwright/utils/socket';
import { MINUTE, SECOND } from 'toolkit/utils/consts';

import Uptime from './Uptime';

const now = new Date('2022-11-11T12:00:00Z').getTime();

const REALTIME_DATA = {
  realtime: {
    end_block: 14416819,
    instant_tps: 213,
    instant_mgas_per_second: 50.91,
    instant_mini_block_interval: 10.53,
    latest_mini_block_id: 1208627478,
    updated_at: '2025-08-19T10:04:53.167705977Z',
  },
  timestamp: 1755597893167,
  type: 'realtime_metrics',
};

const HISTORY_FULL_DATA = {
  data: {
    historical_gas_per_second_3h: [
      { timestamp: (now - 5 * MINUTE) / SECOND, value: 78093010 },
      { timestamp: (now - 4 * MINUTE) / SECOND, value: 120212720 },
      { timestamp: (now - 3 * MINUTE) / SECOND, value: 49049412 },
      { timestamp: (now - 2 * MINUTE) / SECOND, value: 70900270 },
      { timestamp: (now - MINUTE) / SECOND, value: 97225784 },
    ],
    historical_gas_per_second_24h: [],
    historical_gas_per_second_7d: [],
    historical_tps_3h: [
      { timestamp: (now - 5 * MINUTE) / SECOND, value: 196 },
      { timestamp: (now - 4 * MINUTE) / SECOND, value: 203.2 },
      { timestamp: (now - 3 * MINUTE) / SECOND, value: 197.6 },
      { timestamp: (now - 2 * MINUTE) / SECOND, value: 215.4 },
      { timestamp: (now - MINUTE) / SECOND, value: 218.2 },
    ],
    historical_tps_24h: [],
    historical_tps_7d: [],
    historical_mini_block_interval_3h: [
      { timestamp: (now - 5 * MINUTE) / SECOND, value: 10.537408 },
      { timestamp: (now - 4 * MINUTE) / SECOND, value: 10.593221 },
      { timestamp: (now - 3 * MINUTE) / SECOND, value: 10.537408 },
      { timestamp: (now - 2 * MINUTE) / SECOND, value: 10.593221 },
      { timestamp: (now - MINUTE) / SECOND, value: 10.570825 },
    ],
    historical_mini_block_interval_24h: [],
    historical_mini_block_interval_7d: [],
  },
  type: 'history_full',
};

const HISTORY_DELTA_DATA = {
  data: {
    historical_gas_per_second_3h: { timestamp: now / SECOND, value: 87762230 },
    historical_tps_3h: { timestamp: now / SECOND, value: 211.8 },
    historical_mini_block_interval_3h: { timestamp: now / SECOND, value: 10.593221 },
  },
  type: 'history_delta',
};

const WEBSOCKET_URL = `ws://${ config.app.host }:${ socketPort }`;

test('default view', async({ render, createSocket, mockEnvs, page }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_MEGA_ETH_SOCKET_URL_METRICS', WEBSOCKET_URL ],
  ]);
  const component = await render(<Uptime/>, undefined, { withSocket: true });
  const socket = await createSocket();
  socket.send(JSON.stringify(HISTORY_FULL_DATA));
  socket.send(JSON.stringify(REALTIME_DATA));

  await page.waitForFunction(() => {
    return document.querySelector('path[data-name="tps-small"]')?.getAttribute('opacity') === '1';
  });
  await page.waitForFunction(() => {
    return document.querySelector('path[data-name="gas-small"]')?.getAttribute('opacity') === '1';
  });
  await page.waitForFunction(() => {
    return document.querySelector('path[data-name="blockInterval-small"]')?.getAttribute('opacity') === '1';
  });
  await expect(component).toHaveScreenshot();

  socket.send(JSON.stringify(HISTORY_DELTA_DATA));
  await component.locator('.ChartOverlay').first().hover({ position: { x: 350, y: 200 } });

  await expect(component).toHaveScreenshot();
});
