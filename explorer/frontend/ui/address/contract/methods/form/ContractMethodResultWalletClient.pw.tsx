import React from 'react';

import { test, expect } from 'playwright/lib';

import type { PropsDumb } from './ContractMethodResultWalletClient';
import { ContractMethodResultWalletClientDumb } from './ContractMethodResultWalletClient';

test('loading', async({ render }) => {
  const props = {
    txInfo: {
      status: 'pending' as const,
      error: null,
    } as PropsDumb['txInfo'],
    data: {
      hash: '0x363574E6C5C71c343d7348093D84320c76d5Dd29' as `0x${ string }`,
    },
    onSettle: () => {},
  };

  const component = await render(<ContractMethodResultWalletClientDumb { ...props }/>);
  await expect(component).toHaveScreenshot();
});

test('success', async({ render }) => {
  const props = {
    txInfo: {
      status: 'success' as const,
      error: null,
    } as PropsDumb['txInfo'],
    data: {
      hash: '0x363574E6C5C71c343d7348093D84320c76d5Dd29' as `0x${ string }`,
    },
    onSettle: () => {},
  };

  const component = await render(<ContractMethodResultWalletClientDumb { ...props }/>);
  await expect(component).toHaveScreenshot();
});

test('error +@mobile', async({ render }) => {
  const props = {
    txInfo: {
      status: 'error' as const,
      error: {
        // eslint-disable-next-line max-len
        message: 'missing revert data in call exception; Transaction reverted without a reason string [ See: https://links.ethers.org/v5-errors-CALL_EXCEPTION ]',
      } as Error,
    } as PropsDumb['txInfo'],
    data: {
      hash: '0x363574E6C5C71c343d7348093D84320c76d5Dd29' as `0x${ string }`,
    },
    onSettle: () => {},
  };

  const component = await render(<ContractMethodResultWalletClientDumb { ...props }/>);
  await expect(component).toHaveScreenshot();
});

test('error in result', async({ render }) => {
  const props = {
    txInfo: {
      status: 'idle' as const,
      error: null,
    } as unknown as PropsDumb['txInfo'],
    data: {
      message: 'wallet is not connected',
    } as Error,
    onSettle: () => {},
  };

  const component = await render(<ContractMethodResultWalletClientDumb { ...props }/>);
  await expect(component).toHaveScreenshot();
});
