import type { AddressCoinBalanceHistoryItem, AddressCoinBalanceHistoryResponse, AddressCoinBalanceHistoryChart } from 'types/api/address';

export const base: AddressCoinBalanceHistoryItem = {
  block_number: 30367643,
  block_timestamp: '2022-12-11T17:55:20Z',
  delta: '-5568096000000000',
  transaction_hash: null,
  value: '107014805905725000000',
};

export const baseResponse: AddressCoinBalanceHistoryResponse = {
  items: [
    {
      block_number: 30367643,
      block_timestamp: '2022-10-11T17:55:20Z',
      delta: '-2105682233848856',
      transaction_hash: null,
      value: '10102109526582662088',
    },
    {
      block_number: 30367234,
      block_timestamp: '2022-10-01T17:55:20Z',
      delta: '1933020674364000',
      transaction_hash: null,
      value: '10143933697708939226',
    },
    {
      block_number: 30363402,
      block_timestamp: '2022-09-03T17:55:20Z',
      delta: '-1448410607186694',
      transaction_hash: null,
      value: '10142485287101752532',
    },
  ],
  next_page_params: null,
};

export const chartResponse: AddressCoinBalanceHistoryChart = {
  items: [
    {
      date: '2022-11-02',
      value: '128238612887883515',
    },
    {
      date: '2022-11-03',
      value: '199807583157570922',
    },
    {
      date: '2022-11-04',
      value: '114487912907005778',
    },
    {
      date: '2022-11-05',
      value: '219533112907005778',
    },
    {
      date: '2022-11-06',
      value: '116487912907005778',
    },
    {
      date: '2022-11-07',
      value: '199807583157570922',
    },
    {
      date: '2022-11-08',
      value: '216488112907005778',
    },
  ],
  days: 10,
};
