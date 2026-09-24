import * as crossChainConfigMock from 'mocks/crossChain/config';
import * as crossChainTransfersMock from 'mocks/crossChain/transfers';
import * as crossChainTxMock from 'mocks/crossChain/txs';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TxCrossChainDetailsLifecycle from './TxCrossChainDetailsLifecycle';

test.beforeEach(async({ mockEnvs, mockAssetResponse }) => {
  await mockEnvs([
    ...ENVS_MAP.crossChainTxs,
    [ 'NEXT_PUBLIC_NETWORK_ID', crossChainConfigMock.config[0].id ],
  ]);
  await mockAssetResponse(crossChainConfigMock.config[0].logo as string, './playwright/mocks/duck.png');
  await mockAssetResponse(crossChainConfigMock.config[1].logo as string, './playwright/mocks/goose.png');
  await mockAssetResponse(crossChainTransfersMock.transferA.source_token.icon_url as string, './playwright/mocks/image_s.jpg');
});

test('successful tx +@mobile +@dark-mode', async({ render }) => {

  const component = await render(<TxCrossChainDetailsLifecycle data={ crossChainTxMock.base }/>);

  await component.getByRole('button', { name: 'Initiated' }).click();
  await component.getByRole('button', { name: 'Completed' }).click();

  await expect(component).toHaveScreenshot();
});

test('initiated tx', async({ render }) => {
  const component = await render(<TxCrossChainDetailsLifecycle data={ crossChainTxMock.pending }/>);
  await component.getByRole('button', { name: 'Initiated' }).click();

  await expect(component).toHaveScreenshot();
});

test('failed on first step', async({ render }) => {
  const component = await render(<TxCrossChainDetailsLifecycle data={{
    ...crossChainTxMock.failed,
    destination_transaction_hash: undefined,
  }}/>);
  await component.getByRole('button', { name: 'Initiated' }).click();

  await expect(component).toHaveScreenshot();
});
