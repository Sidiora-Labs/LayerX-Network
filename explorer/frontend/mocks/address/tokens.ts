import type { AddressCollectionsResponse, AddressNFTsResponse, AddressTokenBalance, AddressTokensResponse } from 'types/api/address';

import * as tokens from 'mocks/tokens/tokenInfo';
import * as tokenInstance from 'mocks/tokens/tokenInstance';

export const erc20a: AddressTokenBalance = {
  token: tokens.tokenInfoERC20a,
  token_id: null,
  value: '1169321234567891234567891',
  token_instance: null,
};

export const erc20b: AddressTokenBalance = {
  token: tokens.tokenInfoERC20b,
  token_id: null,
  value: '872500000000',
  token_instance: null,
};

export const erc20c: AddressTokenBalance = {
  token: tokens.tokenInfoERC20c,
  token_id: null,
  value: '9852000000000000000000',
  token_instance: null,
};

export const erc20d: AddressTokenBalance = {
  token: tokens.tokenInfoERC20d,
  token_id: null,
  value: '39000000000000000000',
  token_instance: null,
};

export const erc20LongSymbol: AddressTokenBalance = {
  token: tokens.tokenInfoERC20LongSymbol,
  token_id: null,
  value: '39000000000000000000',
  token_instance: null,
};

export const erc20BigAmount: AddressTokenBalance = {
  token: {
    ...tokens.tokenInfoERC20LongSymbol,
    exchange_rate: '4200000000',
    name: 'DuckDuckGoose Stable Coin',
  },
  token_id: null,
  value: '39000000000000000000',
  token_instance: null,
};

export const erc721a: AddressTokenBalance = {
  token: tokens.tokenInfoERC721a,
  token_id: null,
  value: '51',
  token_instance: null,
};

export const erc721b: AddressTokenBalance = {
  token: tokens.tokenInfoERC721b,
  token_id: null,
  value: '1',
  token_instance: null,
};

export const erc721c: AddressTokenBalance = {
  token: tokens.tokenInfoERC721c,
  token_id: null,
  value: '5',
  token_instance: null,
};

export const erc721LongSymbol: AddressTokenBalance = {
  token: tokens.tokenInfoERC721LongSymbol,
  token_id: null,
  value: '5',
  token_instance: null,
};

export const erc1155a: AddressTokenBalance = {
  token: tokens.tokenInfoERC1155a,
  token_id: '42',
  token_instance: tokenInstance.base,
  value: '24',
};

export const erc1155b: AddressTokenBalance = {
  token: tokens.tokenInfoERC1155b,
  token_id: '100010000000001',
  token_instance: tokenInstance.base,
  value: '11',
};

export const erc1155withoutName: AddressTokenBalance = {
  token: tokens.tokenInfoERC1155WithoutName,
  token_id: '64532245',
  token_instance: tokenInstance.base,
  value: '42',
};

export const erc1155LongId: AddressTokenBalance = {
  token: tokens.tokenInfoERC1155b,
  token_id: '483200961027732618117991942553110860267520',
  token_instance: tokenInstance.base,
  value: '42',
};

export const erc404a: AddressTokenBalance = {
  token: tokens.tokenInfoERC404,
  token_id: '42',
  token_instance: tokenInstance.base,
  value: '240000000000000',
};

export const erc404b: AddressTokenBalance = {
  token: tokens.tokenInfoERC404,
  token_instance: null,
  value: '11',
  token_id: null,
};

export const erc20List: AddressTokensResponse = {
  items: [
    erc20a,
    erc20b,
    erc20c,
  ],
  next_page_params: null,
};

export const erc721List: AddressTokensResponse = {
  items: [
    erc721a,
    erc721b,
    erc721c,
  ],
  next_page_params: null,
};

export const erc1155List: AddressTokensResponse = {
  items: [
    erc1155withoutName,
    erc1155a,
    erc1155b,
  ],
  next_page_params: null,
};

export const erc404List: AddressTokensResponse = {
  items: [
    erc404a,
    erc404b,
  ],
  next_page_params: null,
};

export const nfts: AddressNFTsResponse = {
  items: [
    {
      ...tokenInstance.base,
      token: tokens.tokenInfoERC1155a,
      token_type: 'ERC-1155',
      value: '11',
    },
    {
      ...tokenInstance.unique,
      token: tokens.tokenInfoERC721a,
      token_type: 'ERC-721',
      value: '1',
    },
    {
      ...tokenInstance.unique,
      token: tokens.tokenInfoERC404,
      token_type: 'ERC-404',
      value: '11000',
    },
  ],
  next_page_params: null,
};

const nftInstance = {
  ...tokenInstance.base,
  token_type: 'ERC-1155',
  value: '11',
};

const nftInstanceWithoutImage = {
  ...nftInstance,
  image_url: null,
};

export const collections: AddressCollectionsResponse = {
  items: [
    {
      token: tokens.tokenInfoERC1155a,
      amount: '100',
      token_instances: Array(5).fill(nftInstanceWithoutImage),
    },
    {
      token: tokens.tokenInfoERC20LongSymbol,
      amount: '100',
      token_instances: Array(5).fill(nftInstanceWithoutImage),
    },
    {
      token: tokens.tokenInfoERC1155WithoutName,
      amount: '1',
      token_instances: [ nftInstanceWithoutImage ],
    },
  ],
  next_page_params: {
    token_contract_address_hash: '123',
    token_type: 'ERC-1155',
  },
};
