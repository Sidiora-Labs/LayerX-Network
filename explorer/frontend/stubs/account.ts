import type { AddressTag, TransactionTag, ApiKey, CustomAbi, VerifiedAddress, TokenInfoApplication, WatchlistAddress } from 'types/api/account';

import { ADDRESS_PARAMS, ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';

export const PRIVATE_TAG_ADDRESS: AddressTag = {
  address: ADDRESS_PARAMS,
  address_hash: ADDRESS_HASH,
  id: 4,
  name: 'placeholder',
};

export const PRIVATE_TAG_TX: TransactionTag = {
  id: 1,
  name: 'placeholder',
  transaction_hash: TX_HASH,
};

export const WATCH_LIST_ITEM_WITH_TOKEN_INFO: WatchlistAddress = {
  address: ADDRESS_PARAMS,
  address_balance: '7072643779453701031672',
  address_hash: ADDRESS_HASH,
  exchange_rate: '0.00099052',
  id: 18,
  name: 'placeholder',
  notification_methods: {
    email: false,
  },
  notification_settings: {
    'ERC-20': {
      incoming: true,
      outcoming: true,
    },
    'ERC-721': {
      incoming: true,
      outcoming: true,
    },
    'ERC-404': {
      incoming: true,
      outcoming: true,
    },
    'native': {
      incoming: true,
      outcoming: true,
    },
  },
  tokens_count: 42,
  tokens_fiat_value: '12345',
  tokens_overflow: false,
};

export const API_KEY: ApiKey = {
  api_key: '9c3ecf44-a1ca-4ff1-b28e-329e8b65f652',
  name: 'placeholder',
};

export const CUSTOM_ABI: CustomAbi = {
  abi: [
    {
      constant: false,
      payable: false,
      inputs: [ { name: 'target', type: 'address' } ],
      name: 'unknownWriteMethod',
      outputs: [ { name: 'result', type: 'address' } ],
      stateMutability: 'nonpayable',
      type: 'function',
    },
  ],
  contract_address: ADDRESS_PARAMS,
  contract_address_hash: ADDRESS_HASH,
  id: 1,
  name: 'placeholder',
};

export const VERIFIED_ADDRESS: VerifiedAddress = {
  userId: 'john.doe@gmail.com',
  chainId: '5',
  contractAddress: ADDRESS_HASH,
  verifiedDate: '2022-11-11',
  metadata: {
    tokenName: 'Placeholder Token',
    tokenSymbol: 'PLC',
  },
};

export const TOKEN_INFO_APPLICATION: TokenInfoApplication = {
  id: '1',
  tokenAddress: ADDRESS_HASH,
  status: 'IN_PROCESS',
  updatedAt: '2022-11-11 13:49:48.031453Z',
  requesterName: 'John Doe',
  requesterEmail: 'john.doe@gmail.com',
  projectWebsite: 'http://example.com',
  projectEmail: 'info@example.com',
  iconUrl: 'https://example.com/100/100',
  projectDescription: 'Hello!',
  projectSector: 'DeFi',
};
