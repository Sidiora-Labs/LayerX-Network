import React from 'react';

import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import TxsTable from './TxsTable';

test('base view +@dark-mode', async({ render }) => {
  const component = await render(
    <TxsTable
      txs={ [ txMock.base, txMock.withPendingUpdate ] }
      sort="default"
      // eslint-disable-next-line react/jsx-no-bind
      onSortToggle={ () => {} }
      top={ 0 }
      showBlockInfo
    />,
  );

  await component.getByText('kitty').first().hover();

  await expect(component).toHaveScreenshot();
});

test.describe('screen xl', () => {
  test.use({ viewport: pwConfig.viewport.xl });

  test('base view', async({ render }) => {
    const component = await render(
      <TxsTable
        txs={ [ txMock.base, txMock.withWatchListNames ] }
        sort="default"
        // eslint-disable-next-line react/jsx-no-bind
        onSortToggle={ () => {} }
        top={ 0 }
        showBlockInfo
      />,
    );

    await component.getByText('kitty').first().hover();

    await expect(component).toHaveScreenshot();
  });
});
