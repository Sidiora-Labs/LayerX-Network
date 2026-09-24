import React from 'react';

import * as txStateMock from 'mocks/txs/state';
import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';

import TxState from './TxState';
import type { TxQuery } from './useTxQuery';

const hooksConfig = {
  router: {
    query: { hash: txMock.base.hash },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:tx_state_changes', txStateMock.baseResponse, { pathParams: { hash: txMock.base.hash } });
  const txQuery = {
    data: txMock.base,
    isPlaceholderData: false,
    isError: false,
  } as TxQuery;
  const component = await render(<TxState txQuery={ txQuery }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
