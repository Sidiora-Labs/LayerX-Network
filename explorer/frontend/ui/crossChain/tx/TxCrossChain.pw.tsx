import * as crossChainConfigMock from 'mocks/crossChain/config';
import * as crossChainTransfersMock from 'mocks/crossChain/transfers';
import * as crossChainTxMock from 'mocks/crossChain/txs';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TxCrossChain from './TxCrossChain';

const hooksConfig = {
  router: {
    query: { id: crossChainTxMock.base.message_id },
  },
};

test('successful tx +@mobile', async({ render, mockEnvs, mockApiResponse, mockAssetResponse, mockTextAd }) => {
  await mockTextAd();
  await mockEnvs([
    ...ENVS_MAP.crossChainTxs,
    [ 'NEXT_PUBLIC_NETWORK_ID', crossChainConfigMock.config[0].id ],
  ]);
  await mockApiResponse('interchainIndexer:message', crossChainTxMock.base, {
    pathParams: {
      id: crossChainTxMock.base.message_id,
    },
  });

  await mockAssetResponse(crossChainConfigMock.config[0].logo as string, './playwright/mocks/duck.png');
  await mockAssetResponse(crossChainConfigMock.config[1].logo as string, './playwright/mocks/goose.png');
  await mockAssetResponse(crossChainTransfersMock.transferA.source_token.icon_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(<TxCrossChain/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});
