import type { GetMessagesResponse, InterchainMessage } from '@blockscout/interchain-indexer-types';
import { MessageStatus } from '@blockscout/interchain-indexer-types';

import { config } from './config';
import { transferA, transferB } from './transfers';

/* eslint-disable max-len */
export const base = {
  bridge: {
    name: 'Avalanche ICTT',
    ui_url: 'https://app.avax.network/',
  },
  message_id: '0x057b42bbbfbb4900e155a554ae67632cb21e6f5a64d815fcad7f33abe552c059',
  status: MessageStatus.MESSAGE_STATUS_COMPLETED,
  source_chain: config[0],
  send_timestamp: '2022-01-13T12:06:24.000Z',
  sender: {
    hash: '0x33a31e0f62c0ddf25090b61ef21a70d5f48725b7',
  },
  source_transaction_hash: '0x866a70cb1c8c33d259c819473d7b419c0de67770755bf07dee14dd2d0c6dc8ab',
  destination_chain: config[1],
  receive_timestamp: '2022-02-13T12:06:30.000Z',
  recipient: {
    hash: '0x012cb6651cb29c7d5dc96173756a773f7fb87cfb',
    ens_domain_name: 'duck-duck.eth',
  },
  destination_transaction_hash: '0xdbdf690cfde8af2ee855bb90bfa9977a2d8ba36c9ae1a2010c67fe3774832213',
  payload: '0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000040000000000000000000000000d7e63822d0e386fb65f28f41e8c1aa8d844ba0180000000000000000000000000000000000000000000000000712d17312044000',
  extra: {},
  transfers: [
    transferA,
    transferB,
  ],
} satisfies InterchainMessage;

export const pending = {
  ...base,
  message_id: '0x057b42bbbfbb4900e155a554ae67632cb21e6f5a64d815fcad7f33abe552c05a',
  status: MessageStatus.MESSAGE_STATUS_INITIATED,
  source_chain: config[1],
  destination_chain: config[2],
  destination_transaction_hash: undefined,
  transfers: [
    transferA,
  ],
} satisfies InterchainMessage;

export const failed = {
  ...base,
  message_id: '0x057b42bbbfbb4900e155a554ae67632cb21e6f5a64d815fcad7f33abe552c05b',
  status: MessageStatus.MESSAGE_STATUS_FAILED,
  source_chain: {
    id: '420',
    name: 'Unknown chain',
    logo: undefined,
    explorer_url: undefined,
  },
  transfers: [
    transferB,
  ],
  bridge: {
    name: 'Goose bridge',
  },
} satisfies InterchainMessage;

export const listResponse = {
  items: [
    base,
    pending,
    failed,
  ],
  next_page_params: {
    page_token: '1',
  },
} satisfies GetMessagesResponse;
