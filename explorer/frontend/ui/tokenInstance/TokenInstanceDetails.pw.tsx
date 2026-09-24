import React from 'react';

import type { AddressMetadataInfo, AddressMetadataTagApi } from 'types/api/addressMetadata';

import config from 'configs/app';
import * as addressMock from 'mocks/address/address';
import { protocolTagWithMeta } from 'mocks/metadata/address';
import { tokenInfoERC721a } from 'mocks/tokens/tokenInfo';
import * as tokenInstanceMock from 'mocks/tokens/tokenInstance';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';
import { MetadataUpdateProvider } from 'ui/tokenInstance/contexts/metadataUpdate';

import TokenInstanceDetails from './TokenInstanceDetails';

const hash = tokenInfoERC721a.address_hash;

const addressMetadataQueryParams = {
  addresses: [ hash ],
  chainId: config.chain.id,
  tagsLimit: '20',
};

function generateAddressMetadataResponse(tag: AddressMetadataTagApi) {
  return {
    addresses: {
      [ hash.toLowerCase() as string ]: {
        tags: [ {
          ...tag,
          meta: JSON.stringify(tag.meta),
        } ],
      },
    },
  } as AddressMetadataInfo;
}

test.beforeEach(async({ mockApiResponse, mockAssetResponse }) => {
  await mockApiResponse('general:address', addressMock.contract, { pathParams: { hash } });
  await mockApiResponse('general:token_instance_transfers_count', { transfers_count: 42 }, { pathParams: { id: tokenInstanceMock.unique.id, hash } });
  await mockAssetResponse('http://localhost:3000/nft-marketplace-logo.png', './playwright/mocks/image_s.jpg');
  await mockAssetResponse(tokenInstanceMock.unique.image_url as string, './playwright/mocks/image_md.jpg');
});

test('base view +@dark-mode', async({ render, page }) => {
  const component = await render(
    <MetadataUpdateProvider>
      <TokenInstanceDetails data={{ ...tokenInstanceMock.unique, image_url: null }} token={ tokenInfoERC721a }/>
    </MetadataUpdateProvider>,
  );
  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});

test.describe('action button', () => {
  test.beforeEach(async({ mockApiResponse, mockAssetResponse }) => {
    const meta = { ...protocolTagWithMeta.meta, appMarketplaceURL: undefined };
    const metadataResponse = generateAddressMetadataResponse({ ...protocolTagWithMeta, meta });
    await mockApiResponse('metadata:info', metadataResponse, { queryParams: addressMetadataQueryParams });
    await mockAssetResponse(protocolTagWithMeta?.meta?.appLogoURL as string, './playwright/mocks/image_s.jpg');
  });

  test('base view +@dark-mode', async({ render, page }) => {
    const component = await render(
      <MetadataUpdateProvider>
        <TokenInstanceDetails data={ tokenInstanceMock.unique } token={ tokenInfoERC721a }/>
      </MetadataUpdateProvider>,
    );
    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });

  test('without marketplaces +@dark-mode', async({ render, page, mockEnvs }) => {
    mockEnvs(ENVS_MAP.noNftMarketplaces);
    const component = await render(
      <MetadataUpdateProvider>
        <TokenInstanceDetails data={ tokenInstanceMock.unique } token={ tokenInfoERC721a }/>
      </MetadataUpdateProvider>,
    );
    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('base view', async({ render, page }) => {
    const component = await render(
      <MetadataUpdateProvider>
        <TokenInstanceDetails data={{ ...tokenInstanceMock.unique, image_url: null }} token={ tokenInfoERC721a }/>
      </MetadataUpdateProvider>,
    );
    await expect(component).toHaveScreenshot({
      mask: [ page.locator(pwConfig.adsBannerSelector) ],
      maskColor: pwConfig.maskColor,
    });
  });
});
