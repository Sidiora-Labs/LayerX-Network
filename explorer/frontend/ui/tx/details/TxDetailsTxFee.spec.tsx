// @vitest-environment jsdom

import React from 'react';

import type { TokenInfo } from 'types/api/token';
import type { Transaction } from 'types/api/transaction';

import { base } from 'mocks/txs/tx';
import { render as renderComponent } from 'ui/paxeerX/account/testWrapper';
import { afterAll, describe, expect, it, vi } from 'vitest';
import { within } from 'vitest/lib';

import type TxDetailsTxFee from './TxDetailsTxFee';

const sidiora: TokenInfo = {
  address_hash: '0x21f7b20a555199fa73A238B1a91FD0f549068fEe',
  type: 'ERC-20',
  name: 'Sidiora',
  symbol: 'SID',
  decimals: '6',
  holders_count: null,
  exchange_rate: null,
  total_supply: null,
  icon_url: null,
  circulating_market_cap: null,
  reputation: null,
};

const originalEnvs = window.__envs;

afterAll(() => {
  window.__envs = originalEnvs;
});

const transaction = (fee: Transaction['fee']): Transaction => ({
  ...base,
  fee,
  exchange_rate: null,
  historic_exchange_rate: null,
});

const render = async(props: React.ComponentProps<typeof TxDetailsTxFee>, groupedFees = false, hidden = false) => {
  window.__envs = {
    ...originalEnvs,
    NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL: 'PAX',
    NEXT_PUBLIC_NETWORK_CURRENCY_DECIMALS: '18',
    NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES: String(groupedFees),
    NEXT_PUBLIC_VIEWS_TX_HIDDEN_FIELDS: hidden ? '["tx_fee"]' : '[]',
  };
  vi.resetModules();
  const { 'default': Component } = await import('./TxDetailsTxFee');
  const { container } = renderComponent(<div data-testid="fee"><Component { ...props }/></div>);
  return within(container).getByTestId('fee');
};

describe('TxDetailsTxFee', () => {
  it.each([ false, true ])('renders Sidiora with grouped fees set to %s', async(groupedFees) => {
    const data = transaction({ type: 'actual', value: '1234567', token: sidiora });
    const container = await render({ data, isLoading: false }, groupedFees);

    expect(container.textContent).toBe('Transaction fee1.234567SID');
    expect(container.textContent).not.toContain('View details');
    expect(container.textContent).not.toContain('PAX');
    expect(container.textContent).not.toContain('Gwei');
  });

  it('retains the smallest Sidiora base unit', async() => {
    const data = transaction({ type: 'actual', value: '1', token: sidiora });

    expect((await render({ data, isLoading: false })).textContent).toBe('Transaction fee0.000001SID');
  });

  it.each([ false, true ])('keeps Paxeer and absent metadata unchanged with grouped fees set to %s', async(groupedFees) => {
    const fee = { type: 'actual', value: '1234567890123456789' };
    const nativeToken = { ...sidiora, symbol: 'PAX', decimals: '18' };
    const explicit = await render({ data: transaction({ ...fee, token: nativeToken }), isLoading: false }, groupedFees);
    const implicit = await render({ data: transaction(fee), isLoading: false }, groupedFees);
    const expected = `Transaction fee1.234567890123456789\u2009PAX${ groupedFees ? 'View details' : '' }`;

    expect(explicit.textContent).toBe(expected);
    expect(implicit.textContent).toBe(expected);
    expect(explicit.querySelectorAll('a')).toHaveLength(implicit.querySelectorAll('a').length);
  });

  it('keeps the native historical exchange-rate display when fees are grouped', async() => {
    const data = {
      ...transaction({ type: 'actual', value: '1000000000000000000' }),
      exchange_rate: '2',
      historic_exchange_rate: '3',
    };
    const container = await render({ data, isLoading: false }, true);

    expect(container.textContent).toBe('Transaction fee1\u2009PAX$3View details');
  });

  it('respects the hidden fee setting', async() => {
    const data = transaction({ type: 'actual', value: '1234567', token: sidiora });

    expect((await render({ data, isLoading: false }, false, true)).textContent).toBe('');
  });
});
