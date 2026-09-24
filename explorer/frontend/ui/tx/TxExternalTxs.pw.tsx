import React from 'react';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TxExternalTxs from './TxExternalTxs';

const EXT_TX_HASH_1 = '1uwpB95K9ae8yrpxxVXJ27ivvHXqrmy82jsamgNtdWJrYDGkCHsRwd2LKXubrQUzXMaojGxZmHZ85XVJN8EJ3LW8';
const EXT_TX_HASH_2 = '2uwpB95K9ae8yrpxxVXJ27ivvHXqrmy82jsamgNtdWJrYDGkCHsRwd2LKXubrQUzXMaojGxZmHZ85XVJN8EJ3LW8';

test('base view', async({ page, render, mockEnvs, mockAssetResponse }) => {
  await mockEnvs(ENVS_MAP.externalTxs);
  await mockAssetResponse('http://example.url', './playwright/mocks/image_s.jpg');
  await render(<TxExternalTxs data={ [ EXT_TX_HASH_1, EXT_TX_HASH_2 ] }/>);
  await page.getByText('2 Solana txns').hover();
  const popover = page.getByText('Solana transactions');
  await expect(popover).toBeVisible();
  await expect(page.getByText('1uwpB95K9ae8yrpxxVXJ27ivvHXqrmy82jsamgNtd...3LW8')).toBeVisible();
  await expect(page.getByText('2uwpB95K9ae8yrpxxVXJ27ivvHXqrmy82jsamgNtd...3LW8')).toBeVisible();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 500, height: 300 } });
});
