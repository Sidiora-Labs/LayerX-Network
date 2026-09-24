import React from 'react';

import * as txMock from 'mocks/txs/tx';
import { test, expect } from 'playwright/lib';

import TxFee from './TxFee';

test.use({ viewport: { width: 300, height: 100 } });

test('base view', async({ render }) => {
  const component = await render(<TxFee tx={ txMock.base }/>);
  await expect(component).toHaveScreenshot();
});

test('no usd value', async({ render }) => {
  const component = await render(<TxFee tx={ txMock.base } accuracy={ 3 } noUsd/>);
  await expect(component).toHaveScreenshot();
});

test('celo gas token', async({ render, mockAssetResponse }) => {
  await mockAssetResponse(txMock.celoTxn.celo?.gas_token?.icon_url as string, './playwright/mocks/image_svg.svg');
  const component = await render(<TxFee tx={ txMock.celoTxn }/>);
  await expect(component).toHaveScreenshot();
});

test('stability token', async({ render, mockAssetResponse }) => {
  await mockAssetResponse(txMock.stabilityTx.stability_fee?.token.icon_url as string, './playwright/mocks/image_svg.svg');
  const component = await render(<TxFee tx={ txMock.stabilityTx }/>);
  await expect(component).toHaveScreenshot();
});
