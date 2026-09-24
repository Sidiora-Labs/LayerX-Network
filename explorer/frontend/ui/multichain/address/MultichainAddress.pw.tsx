import React from 'react';

import * as addressMock from 'mocks/multichain/address';
import * as chainDataMock from 'mocks/multichain/chains';
import * as portfolioMock from 'mocks/multichain/portfolio';
import * as tokensMock from 'mocks/multichain/tokens';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import MultichainAddress from './MultichainAddress';

const CURRENT_ADDRESS = addressMock.addressA.hash;

const hooksConfig = {
  router: {
    query: { hash: CURRENT_ADDRESS },
  },
};

test('base view', async({ mockApiResponse, render, page, mockMultichainConfig, mockEnvs, mockAssetResponse, mockTextAd }) => {

  await mockMultichainConfig();
  await mockEnvs(ENVS_MAP.multichain);
  await mockTextAd();

  await mockApiResponse('multichainAggregator:address', addressMock.addressA, { pathParams: { hash: CURRENT_ADDRESS } });
  await mockApiResponse('multichainAggregator:address_domains', addressMock.addressDomainsA, { pathParams: { hash: CURRENT_ADDRESS } });
  await mockApiResponse('multichainAggregator:address_portfolio', portfolioMock.base, {
    pathParams: { hash: CURRENT_ADDRESS },
    queryParams: { include_poor_reputation_tokens: false },
  });
  await mockApiResponse('multichainAggregator:address_tokens', {
    items: [ tokensMock.tokenAA, tokensMock.tokenAB, tokensMock.tokenBA, tokensMock.tokenCA, tokensMock.tokenDA ],
    next_page_params: { page_token: '1', page_size: 10 },
  }, { pathParams: { hash: CURRENT_ADDRESS }, queryParams: { type: 'ERC-20,NATIVE', include_poor_reputation_tokens: false } });

  await mockAssetResponse(chainDataMock.chainA.logo as string, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(chainDataMock.chainB.logo as string, './playwright/mocks/image_md.jpg');
  await mockAssetResponse(addressMock.domainProtocols[0].icon_url as string, './playwright/mocks/goose.png');
  await mockAssetResponse(addressMock.domainProtocols[1].icon_url as string, './playwright/mocks/duck.png');

  const component = await render(
    <MultichainAddress/>,
    { hooksConfig },
  );

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
