import React from 'react';

import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';

import TxAdditionalInfo from './TxAdditionalInfo';

test('regular transaction +@dark-mode', async({ render, page }) => {
  const component = await render(<TxAdditionalInfo tx={ txMock.base }/>);
  await component.getByLabel('Transaction info').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 400, height: 650 } });
});

test('regular transaction +@mobile -@default', async({ render, page }) => {
  const component = await render(<TxAdditionalInfo tx={ txMock.base } isMobile/>);
  await component.getByLabel('Transaction info').click();
  await expect(page).toHaveScreenshot();
});

test('blob transaction', async({ render, page }) => {
  const component = await render(<TxAdditionalInfo tx={ txMock.withBlob }/>);
  await component.getByLabel('Transaction info').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 400, height: 650 } });
});
