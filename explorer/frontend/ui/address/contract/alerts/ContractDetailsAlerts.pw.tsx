import React from 'react';

import * as addressMock from 'mocks/address/address';
import * as contractMock from 'mocks/contract/info';
import * as socketServer from 'playwright/fixtures/socketServer';
import { test, expect } from 'playwright/lib';

import ContractDetailsAlerts from './ContractDetailsAlerts.pwstory';

// FIXME
// test cases which use socket cannot run in parallel since the socket server always run on the same port
test.describe.configure({ mode: 'serial' });

test('verified with changed byte code socket', async({ render, createSocket }) => {
  const props = {
    data: { ...contractMock.verified, is_partially_verified: true },
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsAlerts { ...props }/>, undefined, { withSocket: true });
  const socket = await createSocket();
  const channel = await socketServer.joinChannel(socket, 'addresses:' + addressMock.contract.hash.toLowerCase());
  socketServer.sendMessage(socket, channel, 'changed_bytecode', {});

  await expect(component).toHaveScreenshot();
});

test('verified via sourcify', async({ render }) => {
  const props = {
    data: contractMock.verifiedViaSourcify,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsAlerts { ...props }/>, undefined, { withSocket: true });

  await expect(component).toHaveScreenshot();
});

test('verified via eth bytecode db', async({ render }) => {
  const props = {
    data: contractMock.verifiedViaEthBytecodeDb,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsAlerts { ...props }/>, undefined, { withSocket: true });

  await expect(component).toHaveScreenshot();
});

test('with twin address alert +@mobile', async({ render }) => {
  const props = {
    data: contractMock.withTwinAddress,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsAlerts { ...props }/>, undefined, { withSocket: true });

  await expect(component).toHaveScreenshot();
});
