import * as transactionMock from 'mocks/noves/transaction';
import type { NovesFlowViewItem } from 'ui/tx/assetFlows/utils/generateFlowViewData';
import { it, expect } from 'vitest';

import { getActionFromTo, getFromTo, getFromToValue } from './utils';

it('get data for FromTo component from transaction', async() => {
  const result = getFromTo(transactionMock.transaction, transactionMock.transaction.accountAddress);

  expect(result).toEqual({
    text: 'Sent to',
    address: '0xef6595A423c99f3f2821190A4d96fcE4DcD89a80',
  });
});

it('get what type of FromTo component will be', async() => {
  const result = getFromToValue(transactionMock.transaction, transactionMock.transaction.accountAddress);

  expect(result).toEqual('sent');
});

it('get data for FromTo component from flow item', async() => {
  const item: NovesFlowViewItem = {
    action: {
      label: 'Sent',
      amount: '3000',
      flowDirection: 'toRight',
      nft: undefined,
      token: {
        address: '0x1bfe4298796198f8664b18a98640cec7c89b5baa',
        decimals: 18,
        name: 'PQR-Test',
        symbol: 'PQR',
      },
    },
    rightActor: {
      address: '0xdD15D2650387Fb6FEDE27ae7392C402a393F8A37',
      name: null,
    },
    accountAddress: '0xef6595a423c99f3f2821190a4d96fce4dcd89a80',
  };

  const result = getActionFromTo(item);

  expect(result).toEqual({
    text: 'Sent to',
    address: '0xdD15D2650387Fb6FEDE27ae7392C402a393F8A37',
    name: null,
  });
});
