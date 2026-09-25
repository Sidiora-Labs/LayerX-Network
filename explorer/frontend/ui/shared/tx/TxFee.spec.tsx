// @vitest-environment jsdom

import { ChakraProvider } from '@chakra-ui/react';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import type { TokenInfo } from 'types/api/token';
import type { Transaction } from 'types/api/transaction';

import { base } from 'mocks/txs/tx';
import { afterAll, describe, expect, it, vi } from 'vitest';

import type TxFee from './TxFee';

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

const render = async(props: React.ComponentProps<typeof TxFee>) => {
  window.__envs = {
    ...originalEnvs,
    NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL: 'PAX',
    NEXT_PUBLIC_NETWORK_CURRENCY_DECIMALS: '18',
    NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES: 'false',
    NEXT_PUBLIC_VIEWS_TX_HIDDEN_FIELDS: '[]',
  };
  vi.resetModules();
  const { 'default': Component } = await import('./TxFee');
  const { 'default': theme } = await import('toolkit/theme/theme');
  const container = document.createElement('div');
  container.innerHTML = renderToStaticMarkup(<ChakraProvider value={ theme }><Component { ...props }/></ChakraProvider>);
  container.querySelectorAll('style').forEach((style) => style.remove());
  return container;
};

describe('TxFee', () => {
  it.each([
    [ '1234567', '1.234567' ],
    [ '1', '0.000001' ],
    [ '0', '0' ],
    [ '9007199254740993123456', '9,007,199,254,740,993.123456' ],
  ])('renders %s Sidiora base units with six decimals', async(amount, expected) => {
    const tx = transaction({ type: 'actual', value: amount, token: sidiora });
    const container = await render({ tx, accuracy: 0 });

    expect(container.textContent).toBe(`${ expected }SID`);
    expect(container.querySelector('a')?.getAttribute('href')).toBe(`/token/${ sidiora.address_hash }`);
    expect(container.textContent).not.toContain('PAX');
    expect(container.textContent).not.toContain('$');
  });

  it('keeps an explicitly declared Paxeer fee identical to an undeclared native fee', async() => {
    const fee = { type: 'actual', value: '1234567890123456789' };
    const nativeToken = { ...sidiora, symbol: 'PAX', decimals: '18' };
    const explicit = await render({ tx: transaction({ ...fee, token: nativeToken }), accuracy: 0 });
    const implicit = await render({ tx: transaction(fee), accuracy: 0 });

    expect(explicit.textContent).toBe('1.234567890123456789\u2009PAX');
    expect(implicit.textContent).toBe('1.234567890123456789\u2009PAX');
    expect(explicit.querySelector('a')).toBeNull();
  });

  it('defaults absent symbol and decimals to the network coin', async() => {
    const { symbol: _symbol, decimals: _decimals, ...metadata } = sidiora;
    const fee: Transaction['fee'] = { type: 'actual', value: '1000000000000000000', token: metadata };

    expect((await render({ tx: transaction(fee) })).textContent).toBe('1\u2009PAX');
  });

  it('uses token metadata for other decimals as well', async() => {
    const token = { ...sidiora, symbol: 'UNIT', decimals: '2' };
    const tx = transaction({ type: 'actual', value: '12345', token });

    expect((await render({ tx })).textContent).toBe('123.45UNIT');
  });

  it('never uses the network exchange rate for a Sidiora fee', async() => {
    const tx = {
      ...transaction({ type: 'actual', value: '1234567', token: sidiora }),
      exchange_rate: '200',
      historic_exchange_rate: '100',
    };

    expect((await render({ tx, hasExchangeRateToggle: true })).textContent).toBe('1.234567SID');
  });

  it('uses the token exchange rate and respects noUsd and noSymbol', async() => {
    const token = { ...sidiora, exchange_rate: '2' };
    const tx = transaction({ type: 'actual', value: '1000000', token });

    expect((await render({ tx })).textContent).toBe('1SID($2)');
    expect((await render({ tx, noUsd: true, noSymbol: true })).textContent).toBe('1');
  });

  it('preserves the historical exchange-rate toggle for a network fee', async() => {
    const tx = {
      ...transaction({ type: 'actual', value: '1000000000000000000' }),
      exchange_rate: '2',
      historic_exchange_rate: '3',
    };
    const container = await render({ tx, hasExchangeRateToggle: true });

    expect(container.textContent).toBe('1\u2009PAX$3');
    expect(container.querySelector('[data-scope="tooltip"]')).not.toBeNull();
  });
});
