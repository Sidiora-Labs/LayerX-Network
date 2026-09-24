import React from 'react';

import * as txnWithdrawalsMock from 'mocks/arbitrum/txnWithdrawals';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ArbitrumL2TxnWithdrawals from './ArbitrumL2TxnWithdrawals';

const TX_HASH = '0x215382498438cb6532a5e5fb07d664bbf912187866591470d47c3cfbce2dc4a8';

const hooksConfig = {
  router: {
    query: { q: TX_HASH },
  },
};

test('base view +@mobile', async({ render, mockApiResponse, mockEnvs, mockTextAd }) => {
  await mockTextAd();
  await mockEnvs(ENVS_MAP.arbitrumRollup);
  await mockApiResponse(
    'general:arbitrum_l2_txn_withdrawals',
    { items: [ txnWithdrawalsMock.unclaimed, txnWithdrawalsMock.claimed ] },
    { pathParams: { hash: TX_HASH } },
  );

  const component = await render(<ArbitrumL2TxnWithdrawals/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});
