import React from 'react';

import * as tokenInstanceMock from 'mocks/tokens/tokenInstance';
import * as txMock from 'mocks/txs/tx';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import TxInfo from './TxInfo';

test('between addresses +@mobile +@dark-mode', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.base } isLoading={ false }/>);

  await page.getByText('View details').click();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('creating contact', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.withContractCreation } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with token transfer +@mobile', async({ render, page, mockAssetResponse }) => {
  await mockAssetResponse(tokenInstanceMock.base.image_url as string, './playwright/mocks/image_s.jpg');
  const component = await render(<TxInfo data={ txMock.withTokenTransfer } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with decoded revert reason', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.withDecodedRevertReason } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with decoded raw reason', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.withRawRevertReason } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('pending', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.pending } isLoading={ false }/>);

  await page.getByText('View details').click();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

// NOTE: On the screenshot from the test for the mobile device, the scroll overlay is not quite right.
// I checked it manually in the real device, there was not any issue with it
test('with actions uniswap +@mobile +@dark-mode', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.withActionsUniswap } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with blob', async({ render, page }) => {
  const component = await render(<TxInfo data={ txMock.withBlob } isLoading={ false }/>);

  await page.getByText('View details').click();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('l2', async({ render, page, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.optimisticRollup);
  const component = await render(<TxInfo data={ txMock.l2tx } isLoading={ false }/>);
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('without testnet warning', async({ render, page, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_IS_TESTNET', 'false' ],
  ]);
  const component = await render(<TxInfo data={ txMock.l2tx } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('stability customization', async({ render, page, mockEnvs, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.stabilityEnvs);
  await mockAssetResponse(txMock.stabilityTx.stability_fee?.token.icon_url as string, './playwright/mocks/image_s.jpg');
  const component = await render(<TxInfo data={ txMock.stabilityTx } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with grouped fees', async({ render, page, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES', 'true' ],
    [ 'NEXT_PUBLIC_VIEWS_TX_HIDDEN_FIELDS', '["gas_price","gas_fees","burnt_fees"]' ],
    [ 'NEXT_PUBLIC_VIEWS_TX_ADDITIONAL_FIELDS', '["set_max_gas_limit"]' ],
  ]);
  const component = await render(<TxInfo data={ txMock.base } isLoading={ false }/>);

  await component.getByText('View details').first().click();
  await component.getByText('View details').first().click();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('arbitrum L1 status', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  const component = await render(<TxInfo data={ txMock.arbitrumTxn } isLoading={ false }/>);

  const statusElement = component.locator('div').filter({ hasText: 'Processed on rollup' }).nth(2);

  await expect(statusElement).toHaveScreenshot();
});

test('with external txs +@mobile', async({ page, render, mockEnvs, mockApiResponse, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.externalTxs);
  await mockApiResponse('general:tx_external_transactions', [ 'tx1', 'tx2', 'tx3' ], { pathParams: { hash: txMock.base.hash } });
  await mockAssetResponse('http://example.url', './playwright/mocks/image_s.jpg');
  const component = await render(<TxInfo data={ txMock.base } isLoading={ false }/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with interop message in +@mobile', async({ render, page, mockEnvs, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.interop);
  await mockAssetResponse('https://example.com/logo.png', './playwright/mocks/image_s.jpg');
  const component = await render(<TxInfo data={ txMock.withInteropInMessage } isLoading={ false }/>);
  await page.getByText('View details').first().click();
  await expect(page.getByText('Interop status')).toBeVisible();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('with interop message out +@mobile', async({ page, render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.interop);
  await mockAssetResponse('https://example.com/logo.png', './playwright/mocks/image_s.jpg');
  const component = await render(<TxInfo data={ txMock.withInteropOutMessage } isLoading={ false }/>);
  await component.getByText('View details').first().click();
  await expect(component.getByText('Interop status')).toBeVisible();

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
    maxDiffPixels: 20,
  });
});
