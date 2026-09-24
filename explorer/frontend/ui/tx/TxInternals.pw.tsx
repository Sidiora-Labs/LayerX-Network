import React from 'react';

import * as internalTxsMock from 'mocks/txs/internalTxs';
import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';

import TxInternals from './TxInternals';
import type { TxQuery } from './useTxQuery';

const TX_HASH = txMock.base.hash;
const hooksConfig = {
  router: {
    query: { hash: TX_HASH },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:tx_internal_txs', internalTxsMock.baseResponse, { pathParams: { hash: TX_HASH } });
  const txQuery = {
    data: txMock.base,
    isPlaceholderData: false,
    isError: false,
  } as TxQuery;
  const component = await render(<TxInternals txQuery={ txQuery }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
