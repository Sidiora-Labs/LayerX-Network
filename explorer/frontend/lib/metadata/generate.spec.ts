import type { ApiData } from './types';

import type { Route } from 'nextjs-routes';

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import generate from './generate';

interface TestCase<Pathname extends Route['pathname']> {
  title: string;
  route: {
    pathname: Pathname;
    query?: Route['query'];
  };
  apiData?: ApiData<Pathname>;
}

const TEST_CASES = [
  {
    title: 'static route',
    route: {
      pathname: '/txs',
    },
  } as TestCase<'/txs'>,
  {
    title: 'dynamic route',
    route: {
      pathname: '/tx/[hash]',
      query: { hash: '0x12345' },
    },
  } as TestCase<'/tx/[hash]'>,
  {
    title: 'dynamic route with API data',
    route: {
      pathname: '/token/[hash]',
      query: { hash: '0x12345' },
    },
    apiData: {
      symbol_or_name: 'USDT',
      address_hash: '0x12345',
      description: 'USDT is a stablecoin',
      projectName: 'Tether',
      icon_url: 'https://example.com/usdt.png',
      exchange_rate: '1.00',
      name: 'USDT',
      symbol: 'USDT',
    },
  } as TestCase<'/token/[hash]'>,
];

describe('generates correct metadata for:', () => {
  beforeEach(() => {
    // Mock date to a fixed value: 2024-01-01T00:00:00.000Z
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2024-01-01T00:00:00.000Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  TEST_CASES.forEach((testCase) => {
    it(`${ testCase.title }`, () => {
      const result = generate(testCase.route, testCase.apiData);
      expect(result).toMatchSnapshot();
    });
  });
});
