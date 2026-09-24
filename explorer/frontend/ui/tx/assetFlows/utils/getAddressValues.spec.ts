import * as transactionMock from 'mocks/noves/transaction';
import { it, expect } from 'vitest';

import { createAddressValues } from './getAddressValues';

it('creates addresses summary values', async() => {
  const result = createAddressValues(transactionMock.transaction, transactionMock.transaction.classificationData.description);

  expect(result).toEqual([
    {
      match: '0xef326CdAdA59D3A740A76bB5f4F88Fb2',
      type: 'address',
      value: {
        hash: '0xef326CdAdA59D3A740A76bB5f4F88Fb2f1076164',
        is_contract: true,
      },
    },
  ]);
});
