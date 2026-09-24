import type {
  TokenCounters,
  TokenHolder,
  TokenHolders,
  TokenHoldersPagination,
  TokenInfo,
  TokenInstance,
  TokenType,
} from 'types/api/token';
import type { TokenInstanceTransferPagination, TokenInstanceTransferResponse } from 'types/api/tokens';
import type { Erc20TotalPayload, TokenTransfer, TokenTransferPagination, TokenTransferResponse } from 'types/api/tokenTransfer';

import { ADDRESS_PARAMS, ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';
import { generateListStub } from './utils';

export const BLOCK_HASH = '0x8fa7b9e5e5e79deeb62d608db22ba9a5cb45388c7ebb9223ae77331c6080dc70';

export const TOKEN_INFO_ERC_20: TokenInfo = {
  address_hash: ADDRESS_HASH,
  circulating_market_cap: '117629601.61913824',
  decimals: '18',
  exchange_rate: '0.999997',
  holders_count: '16026',
  name: 'Stub Token (goerli)',
  symbol: 'STUB',
  total_supply: '60000000000000000000000',
  type: 'ERC-20',
  icon_url: null,
  reputation: 'ok',
};

export const TOKEN_INFO_ERC_721: TokenInfo = {
  ...TOKEN_INFO_ERC_20,
  circulating_market_cap: null,
  type: 'ERC-721',
};

export const TOKEN_INFO_ERC_1155: TokenInfo = {
  ...TOKEN_INFO_ERC_20,
  circulating_market_cap: null,
  type: 'ERC-1155',
};

export const TOKEN_INFO_ERC_404: TokenInfo = {
  ...TOKEN_INFO_ERC_20,
  circulating_market_cap: null,
  type: 'ERC-404',
};

export const TOKEN_COUNTERS: TokenCounters = {
  token_holders_count: '123456',
  transfers_count: '123456',
};

export const TOKEN_HOLDER_ERC_20: TokenHolder = {
  address: ADDRESS_PARAMS,
  value: '1021378038331138520',
};

export const TOKEN_HOLDER_ERC_1155: TokenHolder = {
  address: ADDRESS_PARAMS,
  token_id: '12345',
  value: '1021378038331138520',
};

export const getTokenHoldersStub = (type?: TokenType, pagination: TokenHoldersPagination | null = null): TokenHolders => {
  switch (type) {
    case 'ERC-721':
      return generateListStub<'general:token_holders'>(TOKEN_HOLDER_ERC_20, 50, { next_page_params: pagination });
    case 'ERC-1155':
      return generateListStub<'general:token_holders'>(TOKEN_HOLDER_ERC_1155, 50, { next_page_params: pagination });
    case 'ERC-404':
      return generateListStub<'general:token_holders'>(TOKEN_HOLDER_ERC_1155, 50, { next_page_params: pagination });
    default:
      return generateListStub<'general:token_holders'>(TOKEN_HOLDER_ERC_20, 50, { next_page_params: pagination });
  }
};

export const getTokenInstanceHoldersStub = (type?: TokenType, pagination: TokenHoldersPagination | null = null): TokenHolders => {
  switch (type) {
    case 'ERC-721':
      return generateListStub<'general:token_instance_holders'>(TOKEN_HOLDER_ERC_20, 10, { next_page_params: pagination });
    case 'ERC-1155':
      return generateListStub<'general:token_instance_holders'>(TOKEN_HOLDER_ERC_1155, 10, { next_page_params: pagination });
    case 'ERC-404':
      return generateListStub<'general:token_instance_holders'>(TOKEN_HOLDER_ERC_1155, 10, { next_page_params: pagination });
    default:
      return generateListStub<'general:token_instance_holders'>(TOKEN_HOLDER_ERC_20, 10, { next_page_params: pagination });
  }
};

export const TOKEN_TRANSFER_ERC_20_TOTAL: Erc20TotalPayload = {
  decimals: '18',
  value: '9851351626684503',
};

export const TOKEN_TRANSFER_ERC_20: TokenTransfer = {
  block_hash: BLOCK_HASH,
  block_number: '123456',
  from: ADDRESS_PARAMS,
  log_index: '4',
  method: 'addLiquidity',
  timestamp: '2022-06-24T10:22:11.000000Z',
  to: ADDRESS_PARAMS,
  token: TOKEN_INFO_ERC_20,
  total: TOKEN_TRANSFER_ERC_20_TOTAL,
  transaction_hash: TX_HASH,
  type: 'token_minting',
};

export const TOKEN_TRANSFER_ERC_721: TokenTransfer = {
  ...TOKEN_TRANSFER_ERC_20,
  total: {
    token_id: '35870',
    token_instance: null,
  },
  token: TOKEN_INFO_ERC_721,
};

export const TOKEN_TRANSFER_ERC_1155: TokenTransfer = {
  ...TOKEN_TRANSFER_ERC_20,
  total: {
    token_id: '35870',
    value: '123',
    decimals: '18',
    token_instance: null,
  },
  token: TOKEN_INFO_ERC_1155,
};

export const TOKEN_TRANSFER_ERC_404: TokenTransfer = {
  ...TOKEN_TRANSFER_ERC_20,
  total: {
    token_id: '35870',
    value: '123',
    decimals: '18',
    token_instance: null,
  },
  token: TOKEN_INFO_ERC_404,
};

export const getTokenTransfersStub = (type?: TokenType, pagination: TokenTransferPagination | null = null): TokenTransferResponse => {
  switch (type) {
    case 'ERC-721':
      return generateListStub<'general:token_transfers'>(TOKEN_TRANSFER_ERC_721, 50, { next_page_params: pagination });
    case 'ERC-1155':
      return generateListStub<'general:token_transfers'>(TOKEN_TRANSFER_ERC_1155, 50, { next_page_params: pagination });
    case 'ERC-404':
      return generateListStub<'general:token_transfers'>(TOKEN_TRANSFER_ERC_404, 50, { next_page_params: pagination });
    default:
      return generateListStub<'general:token_transfers'>(TOKEN_TRANSFER_ERC_20, 50, { next_page_params: pagination });
  }
};

export const getTokenInstanceTransfersStub = (type?: TokenType, pagination: TokenInstanceTransferPagination | null = null): TokenInstanceTransferResponse => {
  switch (type) {
    case 'ERC-721':
      return generateListStub<'general:token_instance_transfers'>(TOKEN_TRANSFER_ERC_721, 10, { next_page_params: pagination });
    case 'ERC-1155':
      return generateListStub<'general:token_instance_transfers'>(TOKEN_TRANSFER_ERC_1155, 10, { next_page_params: pagination });
    case 'ERC-404':
      return generateListStub<'general:token_instance_transfers'>(TOKEN_TRANSFER_ERC_404, 10, { next_page_params: pagination });
    default:
      return generateListStub<'general:token_instance_transfers'>(TOKEN_TRANSFER_ERC_20, 10, { next_page_params: pagination });
  }
};

export const TOKEN_INSTANCE: TokenInstance = {
  token: TOKEN_INFO_ERC_721,
  animation_url: null,
  external_app_url: 'https://vipsland.com/nft/collections/genesis/188882',
  id: '188882',
  image_url: 'https://ipfs.vipsland.com/nft/collections/genesis/188882.gif',
  is_unique: true,
  metadata: {
    attributes: Array(3).fill({ trait_type: 'skin tone', value: 'very light skin tone' }),
    description: '**GENESIS #188882**, **8a77ca1bcaa4036f** :: *844th* generation of *#57806 and #57809* :: **eGenetic Hash Code (eDNA)** = *2822355e953a462d*',
    external_url: 'https://vipsland.com/nft/collections/genesis/188882',
    image: 'https://ipfs.vipsland.com/nft/collections/genesis/188882.gif',
    name: 'GENESIS #188882, 8a77ca1bcaa4036f',
  },
  owner: ADDRESS_PARAMS,
  holder_address_hash: ADDRESS_HASH,
  thumbnails: null,
};
