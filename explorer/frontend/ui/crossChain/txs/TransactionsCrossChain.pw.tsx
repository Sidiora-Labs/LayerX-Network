import * as crossChainConfigMock from 'mocks/crossChain/config';
import * as crossChainTransfersMock from 'mocks/crossChain/transfers';
import * as crossChainTxMock from 'mocks/crossChain/txs';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import TransactionsCrossChain from './TransactionsCrossChain';

test.describe('txs', () => {
  test.slow();

  test.beforeEach(async({ mockEnvs, mockApiResponse, mockAssetResponse }) => {
    await mockEnvs([
      ...ENVS_MAP.crossChainTxs,
      [ 'NEXT_PUBLIC_NETWORK_ID', crossChainConfigMock.config[0].id ],
    ]);
    await mockApiResponse('interchainIndexer:messages', crossChainTxMock.listResponse);
    await mockApiResponse('interchainIndexer:stats_common', { total_messages: 100, total_transfers: 101, timestamp: '2022-01-13T12:00:00.000Z' });
    await mockApiResponse('interchainIndexer:stats_daily', { daily_messages: 42, daily_transfers: 55, date: '2022-01-13' });

    await mockAssetResponse(crossChainConfigMock.config[0].logo as string, './playwright/mocks/duck.png');
    await mockAssetResponse(crossChainConfigMock.config[1].logo as string, './playwright/mocks/goose.png');
    await mockAssetResponse(crossChainTransfersMock.transferA.source_token.icon_url as string, './playwright/mocks/image_s.jpg');
  });

  test.describe('desktop', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('base view +@dark-mode', async({ render }) => {
      const component = await render(<TransactionsCrossChain/>);
      await expect(component).toHaveScreenshot({ timeout: 30_000 });
    });
  });

  test.describe('mobile', () => {
    test.use({ viewport: pwConfig.viewport.mobile });

    test('base view', async({ render }) => {
      const component = await render(<TransactionsCrossChain/>);
      await expect(component).toHaveScreenshot({ timeout: 30_000 });
    });
  });
});
