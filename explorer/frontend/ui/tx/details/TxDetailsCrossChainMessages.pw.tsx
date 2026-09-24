import * as crossChainConfigMock from 'mocks/crossChain/config';
import * as crossChainTransfersMock from 'mocks/crossChain/transfers';
import * as crossChainTxMock from 'mocks/crossChain/txs';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import { Container } from 'ui/shared/DetailedInfo/DetailedInfo';

import TxDetailsCrossChainMessages from './TxDetailsCrossChainMessages';

const txHash = crossChainTxMock.base.source_transaction_hash;

test('base view +@mobile +@dark-mode', async({ render, mockEnvs, mockApiResponse, mockAssetResponse }) => {
  await mockEnvs([
    ...ENVS_MAP.crossChainTxs,
    [ 'NEXT_PUBLIC_NETWORK_ID', crossChainConfigMock.config[0].id ],
  ]);
  await mockApiResponse('interchainIndexer:tx_messages', crossChainTxMock.listResponse, {
    pathParams: { hash: txHash },
  });

  await mockAssetResponse(crossChainConfigMock.config[0].logo as string, './playwright/mocks/duck.png');
  await mockAssetResponse(crossChainConfigMock.config[1].logo as string, './playwright/mocks/goose.png');
  await mockAssetResponse(crossChainTransfersMock.transferA.source_token.icon_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(
    <Container>
      <TxDetailsCrossChainMessages hash={ txHash } isLoading={ false }/>
    </Container>,
  );

  await component.getByText('View details').first().click();
  await component.getByText('View details').last().click();

  await expect(component).toHaveScreenshot();
});
