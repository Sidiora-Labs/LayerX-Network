import React from 'react';

import type { AddressMetadataInfo, AddressMetadataTagApi } from 'types/api/addressMetadata';

import config from 'configs/app';
import * as addressMock from 'mocks/address/address';
import { protocolTagWithMeta } from 'mocks/metadata/address';
import { transaction as novesTransaction } from 'mocks/noves/transaction';
import * as txMock from 'mocks/txs/tx';
import { txInterpretation } from 'mocks/txs/txInterpretation';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TxSubHeading from './TxSubHeading';
import type { TxQuery } from './useTxQuery';

const hash = '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193';

const txQuery = {
  data: txMock.base,
  isPlaceholderData: false,
  isError: false,
} as TxQuery;

const addressMetadataQueryParams = {
  addresses: [ txMock.base.to?.hash as string ],
  chainId: config.chain.id,
  tagsLimit: '20',
};

function generateAddressMetadataResponse(tag: AddressMetadataTagApi) {
  return {
    addresses: {
      [ txMock.base.to?.hash?.toLowerCase() as string ]: {
        tags: [ {
          ...tag,
          meta: JSON.stringify(tag.meta),
        } ],
      },
    },
  } as AddressMetadataInfo;
}

test('no interpretation +@mobile', async({ render }) => {
  const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
  await expect(component).toHaveScreenshot();
});

test.describe('blockscout provider', () => {
  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs(ENVS_MAP.txInterpretation);
  });

  test('with interpretation +@mobile +@dark-mode', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:tx_interpretation', txInterpretation, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and action button +@mobile +@dark-mode', async({ render, mockApiResponse, mockAssetResponse }) => {
    const meta = { ...protocolTagWithMeta.meta, appMarketplaceURL: undefined };
    const metadataResponse = generateAddressMetadataResponse({ ...protocolTagWithMeta, meta });
    await mockApiResponse('metadata:info', metadataResponse, { queryParams: addressMetadataQueryParams });
    await mockAssetResponse(protocolTagWithMeta?.meta?.appLogoURL as string, './playwright/mocks/image_s.jpg');
    await mockApiResponse('general:tx_interpretation', txInterpretation, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and recipient name +@mobile', async({ render, mockApiResponse }) => {
    const newTxQuery = { ...txQuery, data: txMock.withRecipientName } as TxQuery;
    await mockApiResponse('general:tx_interpretation', txInterpretation, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ newTxQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and recipient ENS domain +@mobile', async({ render, mockApiResponse }) => {
    const newTxQuery = { ...txQuery, data: txMock.withRecipientEns } as TxQuery;
    await mockApiResponse('general:tx_interpretation', txInterpretation, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ newTxQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and recipient name tag +@mobile', async({ render, mockApiResponse }) => {
    const newTxQuery = { ...txQuery, data: txMock.withRecipientNameTag } as TxQuery;
    await mockApiResponse('general:tx_interpretation', txInterpretation, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ newTxQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and view all link +@mobile', async({ render, mockApiResponse }) => {
    await mockApiResponse(
      'general:tx_interpretation',
      { data: { summaries: [ ...txInterpretation.data.summaries, ...txInterpretation.data.summaries ] } },
      { pathParams: { hash } },
    );
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('with interpretation and view all link, and action button (external link) +@mobile', async({
    render, mockApiResponse, mockAssetResponse,
  }) => {
    const metadataResponse = generateAddressMetadataResponse(protocolTagWithMeta);
    await mockApiResponse('metadata:info', metadataResponse, { queryParams: addressMetadataQueryParams });
    await mockAssetResponse(protocolTagWithMeta?.meta?.appLogoURL as string, './playwright/mocks/image_s.jpg');
    await mockApiResponse(
      'general:tx_interpretation',
      { data: { summaries: [ ...txInterpretation.data.summaries, ...txInterpretation.data.summaries ] } },
      { pathParams: { hash } },
    );
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('no interpretation, has method called', async({ render, mockApiResponse, mockAssetResponse }) => {
    const newTxQuery = { ...txQuery, data: txMock.withRecipientContract } as TxQuery;
    const metadataResponse = generateAddressMetadataResponse(protocolTagWithMeta);
    await mockApiResponse('metadata:info', metadataResponse, { queryParams: addressMetadataQueryParams });
    await mockAssetResponse(protocolTagWithMeta?.meta?.appLogoURL as string, './playwright/mocks/image_s.jpg');
    await mockApiResponse('general:tx_interpretation', { data: { summaries: [] } }, { pathParams: { hash } });

    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ newTxQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('no interpretation, with action button', async({ render, mockApiResponse, mockAssetResponse }) => {
    const metadataResponse = generateAddressMetadataResponse(protocolTagWithMeta);
    await mockApiResponse('metadata:info', metadataResponse, { queryParams: addressMetadataQueryParams });
    await mockAssetResponse(protocolTagWithMeta?.meta?.appLogoURL as string, './playwright/mocks/image_s.jpg');

    const newTxQuery = { ...txQuery, data: { ...txMock.pending, to: addressMock.contract } } as TxQuery;
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ newTxQuery }/>);
    await expect(component).toHaveScreenshot();
  });

  test('no interpretation (pending)', async({ render, mockApiResponse }) => {
    const txPendingQuery = {
      data: txMock.pending,
      isPlaceholderData: false,
      isError: false,
    } as TxQuery;
    await mockApiResponse('general:tx_interpretation', { data: { summaries: [] } }, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txPendingQuery }/>);
    await expect(component).toHaveScreenshot();
  });
});

test.describe('noves provider', () => {
  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs([ [ 'NEXT_PUBLIC_TRANSACTION_INTERPRETATION_PROVIDER', 'noves' ] ]);
  });

  test('with interpretation +@mobile +@dark-mode', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:noves_transaction', novesTransaction, { pathParams: { hash } });
    const component = await render(<TxSubHeading hash={ hash } hasTag={ false } txQuery={ txQuery }/>);
    await expect(component).toHaveScreenshot();
  });
});
