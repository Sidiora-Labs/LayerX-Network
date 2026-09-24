import React from 'react';

import * as addressMocks from 'mocks/address/address';
import * as inputDataMocks from 'mocks/txs/decodedInputData';
import { test, expect } from 'playwright/lib';

import LogItem from './LogItem';

const TOPICS = [
  '0x3a4ec416703c36a61a4b1f690847f1963a6829eac0b52debd40a23b66c142a56',
  '0x0000000000000000000000000000000000000000000000000000000005001bcf',
  '0xe835d1028984e9e6e7d016b77164eacbcc6cc061e9333c0b37982b504f7ea791',
  null,
];
const DATA = '0x0000000000000000000000000000000000000000000000000070265bf0112cee';

test('with decoded input data +@mobile +@dark-mode', async({ render }) => {
  const component = await render(
    <LogItem
      index={ 42 }
      decoded={ inputDataMocks.withIndexedFields }
      address={{ ...addressMocks.withName, is_verified: true }}
      topics={ TOPICS }
      data={ DATA }
      type="transaction"
      transaction_hash={ null }
      block_timestamp={ null }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('without decoded input data +@mobile', async({ render }) => {
  const component = await render(
    <LogItem
      index={ 42 }
      decoded={ null }
      address={ addressMocks.withoutName }
      topics={ TOPICS }
      data={ DATA }
      type="transaction"
      transaction_hash={ null }
      block_timestamp={ null }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('with default data type', async({ render }) => {
  const component = await render(
    <LogItem
      index={ 42 }
      decoded={ null }
      address={ addressMocks.withoutName }
      topics={ TOPICS }
      data="0x6475636b"
      type="address"
      transaction_hash="0x404bd417203769f968aacb1d66211510db86b81303b0c68283b4eb4572e6845c"
      block_timestamp="2022-02-02T12:00:00Z"
      defaultDataType="UTF-8"
    />,
  );
  await expect(component).toHaveScreenshot();
});
