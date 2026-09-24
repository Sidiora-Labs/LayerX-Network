import { Box } from '@chakra-ui/react';
import React from 'react';

import type { AddressParam } from 'types/api/addressParams';

import * as txMock from 'mocks/txs/tx';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test as base, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import LatestTxs from './LatestTxs';

export const test = base.extend<socketServer.SocketServerFixture>({
  createSocket: socketServer.createSocket,
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('default view', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:homepage_txs', [
      txMock.base,
      txMock.withContractCreation,
      txMock.withTokenTransfer,
      txMock.withWatchListNames,
    ]);

    const component = await render(<LatestTxs/>);
    await expect(component).toHaveScreenshot();
  });
});

test('default view +@dark-mode', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:homepage_txs', [
    txMock.base,
    txMock.withContractCreation,
    txMock.withTokenTransfer,
    txMock.withWatchListNames,
  ]);

  const component = await render(<LatestTxs/>);
  await expect(component).toHaveScreenshot();
});

test.describe('small desktop', () => {
  test.use({ viewport: pwConfig.viewport.md });
  test('one tag', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:homepage_txs', [
      {
        ...txMock.withProtocolTag,
        to: {
          ...txMock.withProtocolTag.to,
          metadata: {
            tags: [ {
              slug: 'aerodrome',
              name: 'Very long protocol name that should be truncated',
              tagType: 'protocol',
              ordinal: 0,
              meta: null,
            } ],
            reputation: null,
          },
        } as AddressParam,
      },
    ]);

    const component = await render(
      <Box maxW="800px">
        <LatestTxs/>
      </Box>,
    );
    await expect(component).toHaveScreenshot();
  });

  test('two or more tags', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:homepage_txs', [
      txMock.withWatchListNames,
    ]);

    const component = await render(
      <Box maxW="800px">
        <LatestTxs/>
      </Box>,
    );
    await expect(component).toHaveScreenshot();
  });
});

test.describe('socket', () => {
  test.describe.configure({ mode: 'serial' });

  const hooksConfig = {
    router: {
      pathname: '/',
      query: {},
    },
  };

  test('new item', async({ render, mockApiResponse, createSocket }) => {
    await mockApiResponse('general:homepage_txs', [
      txMock.base,
      txMock.withContractCreation,
      txMock.withTokenTransfer,
    ]);

    const component = await render(<LatestTxs/>, { hooksConfig }, { withSocket: true });
    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, 'transactions:new_transaction');
    socketServer.sendMessage(socket, channel, 'transaction', { transaction: 1 });
    await expect(component).toHaveScreenshot();
  });
});
