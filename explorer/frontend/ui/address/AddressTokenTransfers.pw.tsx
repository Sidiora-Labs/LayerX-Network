import { Box } from '@chakra-ui/react';
import React from 'react';

import * as tokenTransferMock from 'mocks/tokens/tokenTransfer';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect, devices } from 'playwright/lib';

import AddressTokenTransfers from './AddressTokenTransfers';

const CURRENT_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';
const hooksConfig = {
  router: {
    query: { hash: CURRENT_ADDRESS, tab: 'token_transfers' },
  },
};

// FIXME
// test cases which use socket cannot run in parallel since the socket server always run on the same port
test.describe.configure({ mode: 'serial' });

const tokenTransfersWithPagination = {
  items: [ tokenTransferMock.erc1155A ],
  next_page_params: { block_number: 1, index: 1, items_count: 1 },
};
const tokenTransfersWoPagination = {
  items: [ tokenTransferMock.erc1155A ],
  next_page_params: null,
};

test('with pagination', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
    pathParams: { hash: CURRENT_ADDRESS },
    queryParams: { type: [] },
  });
  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokenTransfers/>
    </Box>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot();
});

test('without pagination', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:address_token_transfers', tokenTransfersWoPagination, {
    pathParams: { hash: CURRENT_ADDRESS },
    queryParams: { type: [] },
  });
  const component = await render(
    <Box pt={{ base: '134px', lg: 6 }}>
      <AddressTokenTransfers/>
    </Box>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('with pagination', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: [] },
    });
    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers/>
      </Box>,
      { hooksConfig },
    );
    await expect(component).toHaveScreenshot();
  });

  test('without pagination', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:address_token_transfers', tokenTransfersWoPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: [] },
    });
    const component = await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers/>
      </Box>,
      { hooksConfig },
    );
    await expect(component).toHaveScreenshot();
  });
});

test.describe('socket', () => {
  test('without overload', async({ render, mockApiResponse, createSocket, page }) => {
    const hooksConfigNoToken = {
      router: {
        query: { hash: CURRENT_ADDRESS, tab: 'token_transfers' },
      },
    };
    await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: [] },
    });
    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers/>
      </Box>,
      { hooksConfig: hooksConfigNoToken },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'token_transfer', { token_transfers: [ tokenTransferMock.erc1155B, tokenTransferMock.erc1155C ] });

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(4);
  });

  test('with overload', async({ render, mockApiResponse, page, createSocket }) => {
    const hooksConfigNoToken = {
      router: {
        query: { hash: CURRENT_ADDRESS, tab: 'token_transfers' },
      },
    };
    await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: [] },
    });
    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers overloadCount={ 2 }/>
      </Box>,
      { hooksConfig: hooksConfigNoToken },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'token_transfer', { token_transfers: [ tokenTransferMock.erc1155B, tokenTransferMock.erc1155C ] });

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);

    const counter = await page.locator('tbody tr:nth-child(1)').textContent();
    expect(counter?.startsWith('1 ')).toBe(true);
  });

  test('without overload, with filters', async({ render, mockApiResponse, page, createSocket }) => {
    const hooksConfigWithFilter = {
      router: {
        query: { hash: CURRENT_ADDRESS, type: 'ERC-1155', tab: 'token_transfers' },
      },
    };
    await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: 'ERC-1155' },
    });

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers/>
      </Box>,
      { hooksConfig: hooksConfigWithFilter },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(socket, channel, 'token_transfer', { token_transfers: [ tokenTransferMock.erc1155B, tokenTransferMock.erc20 ] });

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);
  });

  test('with overload, with filters', async({ render, mockApiResponse, page, createSocket }) => {
    const hooksConfigWithFilter = {
      router: {
        query: { hash: CURRENT_ADDRESS, type: 'ERC-1155', tab: 'token_transfers' },
      },
    };
    await mockApiResponse('general:address_token_transfers', tokenTransfersWithPagination, {
      pathParams: { hash: CURRENT_ADDRESS },
      queryParams: { type: 'ERC-1155' },
    });

    await render(
      <Box pt={{ base: '134px', lg: 6 }}>
        <AddressTokenTransfers overloadCount={ 2 }/>
      </Box>,
      { hooksConfig: hooksConfigWithFilter },
      { withSocket: true },
    );

    const socket = await createSocket();
    const channel = await socketServer.joinChannel(socket, `addresses:${ CURRENT_ADDRESS.toLowerCase() }`);

    const itemsCount = await page.locator('tbody tr').count();
    expect(itemsCount).toBe(2);

    socketServer.sendMessage(
      socket,
      channel,
      'token_transfer',
      { token_transfers: [ tokenTransferMock.erc1155B, tokenTransferMock.erc20, tokenTransferMock.erc1155C, tokenTransferMock.erc721 ] },
    );

    const thirdRow = page.locator('tbody tr:nth-child(3)');
    await thirdRow.waitFor();

    const itemsCountNew = await page.locator('tbody tr').count();
    expect(itemsCountNew).toBe(3);

    const counter = await page.locator('tbody tr:nth-child(1)').textContent();
    expect(counter?.startsWith('1 ')).toBe(true);
  });
});
