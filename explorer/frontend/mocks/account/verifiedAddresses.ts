import type { TokenInfoApplication, TokenInfoApplications, VerifiedAddress, VerifiedAddressResponse } from 'types/api/account';
import type { AddressValidationResponseSuccess } from 'ui/addressVerification/types';

export const SIGNATURE = '0x96491e0cd1b99c14951552361b7f6ff64f41651b5d1c12501914342c8a6847e21e08726c3505e11ba2af9a40ac0b05c8d113e7fd1f74594224b9c7276ebb3a661b';

export const VERIFIED_ADDRESS: Record<string, VerifiedAddress> = {
  NEW_ITEM: {
    userId: '1',
    chainId: '99',
    contractAddress: '0xF822070D07067D1519490dBf49448a7E30EE9ea5',
    verifiedDate: '2022-09-01',
    metadata: {
      tokenName: 'Test Token',
      tokenSymbol: 'TT',
    },
  },
  ITEM_1: {
    userId: '1',
    chainId: '99',
    contractAddress: '0xd0e3010d1ecdbd17aae178b2bf36eb413d8a7441',
    verifiedDate: '2022-08-01',
    metadata: {
      tokenName: 'My Token',
      tokenSymbol: 'MYT',
    },
  },
  ITEM_2: {
    userId: '1',
    chainId: '99',
    contractAddress: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254',
    verifiedDate: '2022-09-23',
    metadata: {
      tokenName: 'Cat Token',
      tokenSymbol: 'CATT',
    },
  },
};

export const ADDRESS_CHECK_RESPONSE = {
  SUCCESS: {
    status: 'SUCCESS',
    result: {
      // eslint-disable-next-line max-len
      signingMessage: '[eth-goerli.blockscout.com] [2023-04-18 18:47:40] I, hereby verify that I am the owner/creator of the address [0xf822070d07067d1519490dbf49448a7e30ee9ea5]',
      contractCreator: '0xd0e3010d1ecdbd17aae178b2bf36eb413d8a7441',
      contractOwner: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254',
    },
  },
  SOURCE_CODE_NOT_VERIFIED_ERROR: {
    status: 'SOURCE_CODE_NOT_VERIFIED_ERROR',
  },
};

export const ADDRESS_VERIFY_RESPONSE: Record<string, AddressValidationResponseSuccess> = {
  SUCCESS: {
    status: 'SUCCESS',
    result: {
      verifiedAddress: VERIFIED_ADDRESS.NEW_ITEM,
    },
  },
  INVALID_SIGNER_ERROR: {
    status: 'INVALID_SIGNER_ERROR',
    invalidSigner: {
      signer: '0xF822070D07067D1519490dBf49448a7E30EE9ea5',
    },
  },
};

export const VERIFIED_ADDRESS_RESPONSE: Record<string, VerifiedAddressResponse> = {
  DEFAULT: {
    verifiedAddresses: [
      VERIFIED_ADDRESS.ITEM_1,
      VERIFIED_ADDRESS.ITEM_2,
    ],
  },
};

export const TOKEN_INFO_APPLICATION_BASE = {
  id: '1',
  tokenAddress: VERIFIED_ADDRESS.ITEM_1.contractAddress,
  status: 'APPROVED',
  updatedAt: '2022-11-08 12:47:10.149148Z',
  requesterName: 'Tom',
  requesterEmail: 'tom@example.com',
  projectName: 'My project',
  projectWebsite: 'http://example.com',
  projectEmail: 'token@example.com',
  iconUrl: 'https://placekitten.com/100',
  projectDescription: 'description',
  projectSector: 'DeFi',
  comment: '',
  docs: 'https://example.com/docs',
  github: 'https://github.com',
  telegram: 'https://telegram.com',
  linkedin: 'https://linkedin.com',
  discord: 'https://discord.com',
  slack: 'https://slack.com',
  twitter: 'https://twitter.com',
  openSea: 'https://opensea.com',
  facebook: 'https://facebook.com',
  medium: 'https://medium.com',
  reddit: 'https://reddit.com',
  support: 'support@example.com',
  coinMarketCapTicker: 'https://coinmarketcap.com',
  coinGeckoTicker: 'https://coingecko.com',
  defiLlamaTicker: 'https://defillama.com',
};

export const TOKEN_INFO_APPLICATION: Record<string, TokenInfoApplication> = {
  APPROVED: {
    ...TOKEN_INFO_APPLICATION_BASE,
    tokenAddress: VERIFIED_ADDRESS.ITEM_1.contractAddress,
    id: '1',
    status: 'APPROVED',
    updatedAt: '2022-11-08 12:47:10.149148Z',
  },
  IN_PROCESS: {
    ...TOKEN_INFO_APPLICATION_BASE,
    tokenAddress: VERIFIED_ADDRESS.ITEM_2.contractAddress,
    id: '2',
    status: 'IN_PROCESS',
    updatedAt: '2022-11-10 08:11:10.149148Z',
  },
  UPDATED_ITEM: {
    ...TOKEN_INFO_APPLICATION_BASE,
    tokenAddress: VERIFIED_ADDRESS.ITEM_1.contractAddress,
    id: '1',
    status: 'IN_PROCESS',
    updatedAt: '2022-11-11 05:11:10.149148Z',
  },
};

export const TOKEN_INFO_APPLICATIONS_RESPONSE: Record<string, TokenInfoApplications> = {
  DEFAULT: {
    submissions: [
      TOKEN_INFO_APPLICATION.APPROVED,
      TOKEN_INFO_APPLICATION.IN_PROCESS,
    ],
  },
  FOR_UPDATE: {
    submissions: [
      {
        ...TOKEN_INFO_APPLICATION.APPROVED,
        status: 'UPDATE_REQUIRED',
      },
      TOKEN_INFO_APPLICATION.IN_PROCESS,
    ],
  },
};

export const TOKEN_INFO_FORM_CONFIG = {
  projectSectors: [
    'Infra & Dev tooling',
    'DeFi',
    'Data',
    'Bridge',
    'NFT',
    'Payments',
    'Faucet',
    'DAO',
    'Games',
    'Wallet',
  ],
};
