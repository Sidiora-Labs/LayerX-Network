import React from 'react';

import * as blobsMock from 'mocks/blobs/blobs';
import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';

import TxBlobs from './TxBlobs';
import type { TxQuery } from './useTxQuery';

const hooksConfig = {
  router: {
    query: { hash: txMock.base.hash },
  },
};

test('base view +@mobile', async({ render, mockApiResponse }) => {
  await mockApiResponse('general:tx_blobs', blobsMock.txBlobs, { pathParams: { hash: txMock.base.hash } });
  const txQuery = {
    data: txMock.base,
    isPlaceholderData: false,
    isError: false,
  } as TxQuery;
  const component = await render(<TxBlobs txQuery={ txQuery }/>, { hooksConfig });
  await expect(component).toHaveScreenshot();
});
