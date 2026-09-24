import { Box } from '@chakra-ui/react';
import React from 'react';

import type { OpWithdrawal } from 'types/api/transaction';

import * as addressMock from 'mocks/address/address';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TxDetailsWithdrawalStatusOptimistic from './TxDetailsWithdrawalStatusOptimistic';

const TX_HASH = '0x7d93a59a228e97d084a635181c3053e324237d07566ec12287eae6da2bcf9456';

const BASE_DATA: OpWithdrawal = {
  l1_transaction_hash: TX_HASH,
  nonce: 1,
  status: 'Waiting for state root',
  portal_contract_address_hash: null,
  msg_sender_address_hash: null,
  msg_target_address_hash: null,
  msg_data: null,
  msg_gas_limit: null,
  msg_nonce_raw: null,
  msg_value: null,
};

test('status=Waiting for state root', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.optimisticRollup);

  const data: OpWithdrawal = {
    ...BASE_DATA,
    status: 'Waiting for state root',
  };

  const component = await render(
    <Box p={ 2 }>
      <TxDetailsWithdrawalStatusOptimistic
        data={ data }
        txHash={ TX_HASH }
        from={ addressMock.withoutName }
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});

test.describe('status=Ready for relay', () => {
  test('with claim button', async({ render, mockEnvs }) => {
    await mockEnvs(ENVS_MAP.optimisticRollup);

    const data: OpWithdrawal = {
      ...BASE_DATA,
      status: 'Ready for relay',
    };

    const component = await render(
      <Box p={ 2 }>
        <TxDetailsWithdrawalStatusOptimistic
          data={ data }
          txHash={ TX_HASH }
          from={ addressMock.withoutName }
        />
      </Box>,
    );

    await expect(component).toHaveScreenshot();
  });

  test('without claim button', async({ render, mockEnvs }) => {
    await mockEnvs([
      ...ENVS_MAP.optimisticRollup,
      [ 'NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL', '' ],
    ]);

    const data: OpWithdrawal = {
      ...BASE_DATA,
      status: 'Ready for relay',
    };

    const component = await render(
      <Box p={ 2 }>
        <TxDetailsWithdrawalStatusOptimistic
          data={ data }
          txHash={ TX_HASH }
          from={ addressMock.withoutName }
        />
      </Box>,
    );

    await expect(component).toHaveScreenshot();
  });
});

test('status=Relayed', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.optimisticRollup);

  const data: OpWithdrawal = {
    ...BASE_DATA,
    status: 'Relayed',
  };

  const component = await render(
    <Box p={ 2 }>
      <TxDetailsWithdrawalStatusOptimistic
        data={ data }
        txHash={ TX_HASH }
        from={ addressMock.withoutName }
      />
    </Box>,
  );

  await expect(component).toHaveScreenshot();
});
