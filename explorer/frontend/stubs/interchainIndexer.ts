import type {
  ChainInfo,
  GetCommonStatisticsResponse,
  GetDailyStatisticsResponse,
  InterchainMessage,
  InterchainTransfer,
} from '@blockscout/interchain-indexer-types';
import { MessageStatus } from '@blockscout/interchain-indexer-types';

import { ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';

const MESSAGE_ID = '0xde5c33c6b3424cec51ea6f5d081f64719d531eec74f2a2408141274c117c5f44';

const CHAIN = {
  id: '8021',
  name: 'Avalanche',
  logo: undefined,
  explorer_url: 'https://avalanche.blockscout.com/',
} satisfies ChainInfo;

export const INTERCHAIN_TRANSFER = {
  bridge: {
    name: 'Avalanche ICTT',
  },
  message_id: MESSAGE_ID,
  status: MessageStatus.MESSAGE_STATUS_COMPLETED,
  source_token: {
    address_hash: ADDRESS_HASH,
    name: 'Wrapped AVAX',
    symbol: 'WAVAX',
    decimals: '18',
  },
  source_amount: '80800000000000000',
  source_transaction_hash: TX_HASH,
  source_chain: CHAIN,
  sender: {
    hash: ADDRESS_HASH,
  },
  send_timestamp: '2026-01-07T18:41:50.000Z',
  destination_token: {
    address_hash: ADDRESS_HASH,
    name: 'Wrapped AVAX',
    symbol: 'WAVAX',
    decimals: '18',
  },
  destination_amount: '80800000000000000',
  destination_transaction_hash: TX_HASH,
  destination_chain: CHAIN,
  recipient: {
    hash: ADDRESS_HASH,
  },
  receive_timestamp: '2026-01-07T18:41:53.000Z',
} satisfies InterchainTransfer;

export const INTERCHAIN_MESSAGE = {
  bridge: {
    name: 'Avalanche ICTT',
  },
  message_id: MESSAGE_ID,
  status: MessageStatus.MESSAGE_STATUS_COMPLETED,
  source_chain: CHAIN,
  send_timestamp: '2026-01-07T18:41:50.000Z',
  sender: {
    hash: ADDRESS_HASH,
  },
  source_transaction_hash: TX_HASH,
  destination_chain: CHAIN,
  receive_timestamp: '2026-01-07T18:41:53.000Z',
  recipient: {
    hash: ADDRESS_HASH,
  },
  destination_transaction_hash: TX_HASH,
  payload: '0x00',
  extra: {},
  transfers: [
    INTERCHAIN_TRANSFER,
  ],
} satisfies InterchainMessage;

export const INTERCHAIN_STATS_DAILY = {
  date: '2026-01-12',
  daily_messages: 75,
  daily_transfers: 75,
} satisfies GetDailyStatisticsResponse;

export const INTERCHAIN_STATS_COMMON = {
  timestamp: '2026-01-12T11:49:59.380Z',
  total_messages: 10823,
  total_transfers: 10822,
} satisfies GetCommonStatisticsResponse;
