import React from 'react';

import * as blockMock from 'mocks/blocks/block';
import * as statsMock from 'mocks/stats/index';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect, devices } from 'playwright/lib';

import Blocks from './Blocks';

const hooksConfig = {
  router: {
    query: { tab: 'blocks' },
    isReady: true,
  },
};

// FIXME
// test cases which use socket cannot run in parallel since the socket server always run on the same port
test.describe.configure({ mode: 'serial' });

test.beforeEach(async({ mockTextAd }) => {
  await mockTextAd();
});

test('base view +@dark-mode', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });
  await mockApiResponse('general:stats', statsMock.base);

  const component = await render(<Blocks/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test('hidden fields', async({ render, mockApiResponse, mockEnvs }) => {
  await mockEnvs([
    ...ENVS_MAP.blockHiddenFields,
    [ 'NEXT_PUBLIC_VIEWS_BLOCK_PENDING_UPDATE_ALERT_ENABLED', 'false' ],
  ]);
  await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });
  await mockApiResponse('general:stats', statsMock.base);

  const component = await render(<Blocks/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });
    await mockApiResponse('general:stats', statsMock.base);

    const component = await render(<Blocks/>, { hooksConfig });

    await expect(component).toHaveScreenshot();
  });

  test('hidden fields', async({ render, mockApiResponse, mockEnvs }) => {
    await mockEnvs([
      ...ENVS_MAP.blockHiddenFields,
      [ 'NEXT_PUBLIC_VIEWS_BLOCK_PENDING_UPDATE_ALERT_ENABLED', 'false' ],
    ]);
    await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });
    await mockApiResponse('general:stats', statsMock.base);

    const component = await render(<Blocks/>, { hooksConfig });

    await expect(component).toHaveScreenshot();
  });
});

test('new item from socket', async({ render, mockApiResponse, createSocket }) => {
  await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });

  const component = await render(<Blocks/>, { hooksConfig }, { withSocket: true });

  const socket = await createSocket();
  const channel = await socketServer.joinChannel(socket, 'blocks:new_block');
  socketServer.sendMessage(socket, channel, 'new_block', {
    average_block_time: '6212.0',
    block: {
      ...blockMock.base,
      height: blockMock.base.height + 1,
      timestamp: '2022-11-11T11:59:58Z',
    },
  });

  await expect(component).toHaveScreenshot();
});

test('socket error', async({ render, mockApiResponse, createSocket }) => {
  await mockApiResponse('general:blocks', blockMock.baseListResponse, { queryParams: { type: 'block' } });

  const component = await render(<Blocks/>, { hooksConfig }, { withSocket: true });

  const socket = await createSocket();
  await socketServer.joinChannel(socket, 'blocks:new_block');
  socket.close();

  await expect(component).toHaveScreenshot();
});
