import React from 'react';

import { test, expect } from 'playwright/lib';

import SocketNewItemsNotice from './SocketNewItemsNotice';

const hooksConfig = {
  router: {
    pathname: '/tx/[hash]',
    query: {},
  },
};

test('2 new items in validated txs list +@dark-mode', async({ render }) => {
  const component = await render(<SocketNewItemsNotice url="/" num={ 2 }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test('connection loss', async({ render }) => {
  const component = await render(<SocketNewItemsNotice url="/" showErrorAlert/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});

test('fetching', async({ render }) => {
  const component = await render(<SocketNewItemsNotice url="/"/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
