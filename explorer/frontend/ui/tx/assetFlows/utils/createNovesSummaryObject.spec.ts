import * as transactionMock from 'mocks/noves/transaction';
import { it, expect } from 'vitest';

import { createNovesSummaryObject } from './createNovesSummaryObject';

it('creates interpretation summary object', async() => {
  const result = createNovesSummaryObject(transactionMock.transaction);

  expect(result).toEqual({
    summary_template: ' Called function \'stake\' on contract{0xef326CdAdA59D3A740A76bB5f4F88Fb2}',
    summary_template_variables: {
      '0xef326CdAdA59D3A740A76bB5f4F88Fb2': {
        type: 'address',
        value: {
          hash: '0xef326CdAdA59D3A740A76bB5f4F88Fb2f1076164',
          is_contract: true,
        },
      },
    },
  });
});
