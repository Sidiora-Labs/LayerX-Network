import { Box } from '@chakra-ui/react';
import type { Locator } from '@playwright/test';
import React from 'react';

import * as txMock from 'mocks/txs/tx';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import AddressTxs from './AddressTxs';

const CURRENT_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

const hooksConfig = {
  router: {
    query: { hash: CURRENT_ADDRESS, tab: 'txs' },
  },
};
const DEFAULT_PAGINATION = { block_number: 1, index: 1, items_count: 1 };

test.describe('base view', () => {
  let component: Locator;

  test.beforeEach(async({ render, mockApiResponse }) => {
    await mockApiResponse(
      'general:address_txs',
      {
        items: [
          txMock.base,
          { ...txMock.base, hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3194' },
        ],
        next_page_params: DEFAULT_PAGINATION,
      },
      { pathParams: { hash: CURRENT_ADDRESS } },
    );
    component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig },
    );
  });

  test('desktop', async({ page }) => {
    await expect(component).toHaveScreenshot();
    await component.locator('button[aria-label="Toggle time format"]').click();
    await page.mouse.move(0, 0);
    await expect(component).toHaveScreenshot();
  });

  test.describe('screen xl', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('base view', async() => {
      test.slow();
      await expect(component).toHaveScreenshot();
    });
  });
});

test.describe('mobile', () => {
  test.use({ viewport: pwConfig.viewport.mobile });

  test.beforeEach(async({ mockApiResponse }) => {
    await mockApiResponse(
      'general:address_txs',
      {
        items: [
          txMock.base,
          { ...txMock.base, hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3194' },
        ],
        next_page_params: DEFAULT_PAGINATION,
      },
      { pathParams: { hash: CURRENT_ADDRESS } },
    );
  });

  test('base view', async({ render }) => {
    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig },
    );
    await expect(component).toHaveScreenshot();
  });

  test('table view', async({ render, mockFeatures }) => {
    await mockFeatures([ [ 'txns_view_exp', 'table_view' ] ]);
    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig },
    );
    await expect(component).toHaveScreenshot();
  });
});

test.describe('socket', () => {
  // FIXME
  // test cases which use socket cannot run in parallel since the socket server always run on the same port
  test.describe.configure({ mode: 'serial' });

  test('with update', async({ render, mockApiResponse, page, createSocket }) => {
    await mockApiResponse(
      'general:address_txs',
      { items: [ txMock.pending ], next_page_params: DEFAULT_PAGINATION },
      { pathParams: { hash: CURRENT_ADDRESS } },
    );

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'transaction', { transactions: [ txMock.base ] });

    const secondRow = page.locator('tbody tr:nth-child(2)');
    await secondRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(2);
  });

  test('with overload', async({ render, mockApiResponse, page, createSocket }) => {
    await mockApiResponse(
      'general:address_txs',
      { items: [ txMock.base ], next_page_params: DEFAULT_PAGINATION },
      { pathParams: { hash: CURRENT_ADDRESS } },
    );

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'transaction', { transactions: [ txMock.base2, txMock.base3, txMock.base4 ] });

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);

    const counter = await page.locator('tbody tr:nth-child(1)').textContent();
    expect(counter?.startsWith('2 ')).toBe(true);
  });

  test('without overload, with filters', async({ render, mockApiResponse, page, createSocket }) => {
    const hooksConfigWithFilter = {
      router: {
        query: { hash: CURRENT_ADDRESS, filter: 'from', tab: 'txs' },
      },
    };

    await mockApiResponse(
      'general:address_txs',
      { items: [ txMock.base ], next_page_params: DEFAULT_PAGINATION },
      { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { filter: 'from' } },
    );

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig: hooksConfigWithFilter },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'transaction', { transactions: [ txMock.base2 ] });

    const secondRow = page.locator('tbody tr:nth-child(2)');
    await secondRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);
  });

  test('with overload, with filters', async({ render, mockApiResponse, page, createSocket }) => {
    const hooksConfigWithFilter = {
      router: {
        query: { hash: CURRENT_ADDRESS, filter: 'from', tab: 'txs' },
      },
    };

    await mockApiResponse(
      'general:address_txs',
      { items: [ txMock.base ], next_page_params: DEFAULT_PAGINATION },
      { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { filter: 'from' } },
    );

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTxs/>
      </Box>,
      { hooksConfig: hooksConfigWithFilter },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'transaction', { transactions: [ txMock.base2, txMock.base3, txMock.base4 ] });

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);

    const counter = await page.locator('tbody tr:nth-child(1)').textContent();
    expect(counter?.startsWith('1 ')).toBe(true);
  });
});
