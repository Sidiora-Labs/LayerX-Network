import type { GetTransfersResponse, InterchainTransfer } from '@blockscout/interchain-indexer-types';
import { MessageStatus } from '@blockscout/interchain-indexer-types';

import { config } from './config';

export const transferA = {
  bridge: {
    name: 'Avalanche ICTT',
    ui_url: 'https://app.avax.network/',
  },
  message_id: '0x057b42bbbfbb4900e155a554ae67632cb21e6f5a64d815fcad7f33abe552c059',
  status: MessageStatus.MESSAGE_STATUS_COMPLETED,
  source_chain: config[0],
  destination_chain: config[1],
  source_token: {
    address_hash: '0x33a31e0f62c0ddf25090b61ef21a70d5f48725b7',
    name: 'Wrapped AVAX',
    symbol: 'WAVAX',
    decimals: '18',
    icon_url: 'https://app.avax.network/logo.svg',
  },
  source_amount: '509700000000000000',
  source_transaction_hash: '0x866a70cb1c8c33d259c819473d7b419c0de67770755bf07dee14dd2d0c6dc8ab',
  sender: {
    hash: '0xd7e63822d0e386fb65f28f41e8c1aa8d844ba018',
    ens_domain_name: 'kitty.kitty.kitty.kitty.cat.eth',
  },
  send_timestamp: '2022-01-13T12:06:24.000Z',
  destination_token: {
    address_hash: '0x012cb6651cb29c7d5dc96173756a773f7fb87cfb',
    name: 'Wrapped AVAX',
    symbol: 'WAVAX',
    decimals: '18',
  },
  destination_amount: '509700000000000000',
  destination_transaction_hash: '0xdbdf690cfde8af2ee855bb90bfa9977a2d8ba36c9ae1a2010c67fe3774832213',
  recipient: {
    hash: '0xd7e63822d0e386fb65f28f41e8c1aa8d844ba018',
  },
  receive_timestamp: '2022-01-13T12:06:30.000Z',
} satisfies InterchainTransfer;

export const transferB = {
  ...transferA,
  source_chain: {
    id: '420',
    name: 'Unknown chain',
    logo: undefined,
    explorer_url: undefined,
  },
  source_transaction_hash: '0x866a70cb1c8c33d259c819473d7b419c0de67770755bf07dee14dd2d0c6dc800',
  source_token: {
    address_hash: '0x33a31e0f62c0ddf25090b61ef21a70d5f48725b7',
    name: 'Circle USD',
    symbol: 'USDC',
    decimals: '6',
  },
  destination_transaction_hash: '0xdbdf690cfde8af2ee855bb90bfa9977a2d8ba36c9ae1a2010c67fe3774832214',
  status: MessageStatus.MESSAGE_STATUS_FAILED,
  bridge: {
    name: 'Optimism Superchain',
  },
} satisfies InterchainTransfer;

export const listResponse = {
  items: [
    transferA,
    transferB,
  ],
  next_page_params: {
    page_token: 'token',
  },
} satisfies GetTransfersResponse;
