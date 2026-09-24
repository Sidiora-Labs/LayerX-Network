import type { Transaction } from 'types/api/transaction';

import { describe, it, expect } from 'vitest';

import sortTxs, { sortTxsFromSocket } from './sortTxs';

describe('sortTxs', () => {
  it('should sort transactions by value in descending order', () => {
    const txs = [
      { value: '42' },
      { value: '11' },
      { value: '24' },
    ] as Array<Transaction>;
    const result = txs.sort(sortTxs('value-desc'));
    expect(result).toEqual([
      { value: '42' },
      { value: '24' },
      { value: '11' },
    ]);
  });

  it('should sort transactions by value in ascending order', () => {
    const txs = [
      { value: '42' },
      { value: '11' },
      { value: '24' },
    ] as Array<Transaction>;
    const result = txs.sort(sortTxs('value-asc'));
    expect(result).toEqual([
      { value: '11' },
      { value: '24' },
      { value: '42' },
    ]);
  });

  it('should sort transactions by fee in descending order', () => {
    const txs = [
      { fee: { value: '42' } },
      { fee: { value: '11' } },
      { fee: { value: '24' } },
    ] as Array<Transaction>;
    const result = txs.sort(sortTxs('fee-desc'));
    expect(result).toEqual([
      { fee: { value: '42' } },
      { fee: { value: '24' } },
      { fee: { value: '11' } },
    ]);
  });

  it('should sort transactions by fee in ascending order', () => {
    const txs = [
      { fee: { value: '42' } },
      { fee: { value: '11' } },
      { fee: { value: '24' } },
    ] as Array<Transaction>;
    const result = txs.sort(sortTxs('fee-asc'));
    expect(result).toEqual([
      { fee: { value: '11' } },
      { fee: { value: '24' } },
      { fee: { value: '42' } },
    ]);
  });
});

describe('sortTxsFromSocket', () => {
  it('should sort transaction by age in ascending order if sorting is not provided', () => {
    const txs = [
      { timestamp: '2022-11-01T12:33:00Z' },
      { timestamp: '2022-11-01T12:00:00Z' },
      { timestamp: null },
      { timestamp: '2022-11-03T03:03:00Z' },
    ] as Array<Transaction>;
    const result = txs.sort(sortTxsFromSocket(undefined));
    expect(result).toEqual([
      { timestamp: null },
      { timestamp: '2022-11-03T03:03:00Z' },
      { timestamp: '2022-11-01T12:33:00Z' },
      { timestamp: '2022-11-01T12:00:00Z' },
    ]);
  });
});
