import React from 'react';

import * as addressMock from 'mocks/multichain/address';
import * as chainDataMock from 'mocks/multichain/chains';
import * as portfolioMock from 'mocks/multichain/portfolio';
import * as tokensMock from 'mocks/multichain/tokens';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import MultichainAddressPortfolioTokens from './MultichainAddressPortfolioTokens';

const CURRENT_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';
const hooksConfig = {
  router: {
    query: { hash: CURRENT_ADDRESS, tab: 'portfolio_tokens' },
  },
};

test.beforeEach(async({ mockMultichainConfig, mockAssetResponse, mockEnvs }) => {
  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/duck.png');
  await mockAssetResponse(chainDataMock.chainB.logo as string, './playwright/mocks/goose.png');
});

test('many chains +@mobile +@dark-mode', async({ render, mockApiResponse, page }) => {
  await mockApiResponse('multichainAggregator:address_portfolio',
    portfolioMock.base,
    { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { include_poor_reputation_tokens: false },
    });
  await mockApiResponse('multichainAggregator:address_tokens', {
    items: [ tokensMock.tokenAA, tokensMock.tokenAB, tokensMock.tokenBA, tokensMock.tokenCA, tokensMock.tokenDA ],
    next_page_params: { page_token: '1', page_size: 10 },
  }, { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { type: 'ERC-20,NATIVE', include_poor_reputation_tokens: false } });
  await mockApiResponse('multichainAggregator:address_tokens', {
    items: [ ],
    next_page_params: undefined,
  }, {
    pathParams: { hash: CURRENT_ADDRESS },
    queryParams: { type: 'ERC-20,NATIVE', chain_id: chainDataMock.chainE.id, include_poor_reputation_tokens: false },
  });

  const component = await render(
    <MultichainAddressPortfolioTokens addressData={ addressMock.addressA } isLoading={ false }/>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });

  await component.getByText('show more').click();
  await component.getByLabel('White goose portfolio selector').click();
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test('zero net worth', async({ render, mockApiResponse, page }) => {
  const addressData = {
    ...addressMock.addressA,
    chain_infos: {
      [chainDataMock.chainD.id]: {
        ...addressMock.addressA.chain_infos[chainDataMock.chainD.id],
      },
    },
  };
  await mockApiResponse('multichainAggregator:address_portfolio',
    portfolioMock.zero,
    { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { include_poor_reputation_tokens: false },
    });
  await mockApiResponse('multichainAggregator:address_tokens', {
    items: [ tokensMock.tokenDA ],
    next_page_params: undefined,
  }, { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { type: 'ERC-20,NATIVE', include_poor_reputation_tokens: false } });

  const component = await render(
    <MultichainAddressPortfolioTokens addressData={ addressData } isLoading={ false }/>,
    { hooksConfig },
  );
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
