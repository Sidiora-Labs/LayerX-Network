import React from 'react';

import * as txMock from 'mocks/txs/tx';
import { test, expect, devices } from 'playwright/lib';

import TxsListItem from './TxsListItem';

test.use({ viewport: devices['iPhone 13 Pro'].viewport });

test('base view +@dark-mode', async({ render }) => {
  const component = await render(<TxsListItem tx={ txMock.withWatchListNames } showBlockInfo/>);
  await expect(component).toHaveScreenshot();
});

test('with base address', async({ render }) => {
  const component = await render(<TxsListItem tx={ txMock.withWatchListNames } showBlockInfo currentAddress={ txMock.base.from.hash }/>);
  await expect(component).toHaveScreenshot();
});

test('with pending update', async({ render }) => {
  const component = await render(<TxsListItem tx={ txMock.withPendingUpdate } showBlockInfo/>);
  await expect(component).toHaveScreenshot();
});
