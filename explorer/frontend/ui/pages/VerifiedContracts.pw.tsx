import React from 'react';

import { verifiedContractsCountersMock } from 'mocks/contracts/counters';
import * as verifiedContractsMock from 'mocks/contracts/index';
import { test, expect } from 'playwright/lib';

import VerifiedContracts from './VerifiedContracts';

test('base view +@mobile', async({ render, mockTextAd, mockApiResponse, mockEnvs }) => {
  test.slow();
  await mockEnvs([ [ 'NEXT_PUBLIC_STATS_API_HOST', '' ] ]);
  await mockTextAd();
  await mockApiResponse('general:verified_contracts', verifiedContractsMock.baseResponse);
  await mockApiResponse('general:verified_contracts_counters', verifiedContractsCountersMock);
  await mockApiResponse('general:config_contract_languages', { languages: [ 'solidity', 'vyper' ] });
  const component = await render(<VerifiedContracts/>);
  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});
