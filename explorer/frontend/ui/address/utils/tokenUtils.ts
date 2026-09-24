import BigNumber from 'bignumber.js';

import type { AddressTokenBalance } from 'types/api/address';

import config from 'configs/app';
import sumBnReducer from 'lib/bigint/sumBnReducer';
import { isFungibleTokenType } from 'lib/token/tokenTypes';
import { ZERO } from 'toolkit/utils/consts';

const isNativeToken = (token: TokenEnhancedData) =>
  config.UI.views.address.nativeTokenAddress &&
  token.token.address_hash.toLowerCase() === config.UI.views.address.nativeTokenAddress.toLowerCase();

export type TokenEnhancedData = AddressTokenBalance & {
  usd?: BigNumber ;
  chain_values?: Record<string, string>;
};

export type Sort = 'desc' | 'asc';

export type TokenSelectData = Record<string, TokenSelectDataItem>;

export interface TokenSelectDataItem {
  items: Array<TokenEnhancedData>;
  isOverflow: boolean;
}

type TokenGroup = [string, TokenSelectDataItem];

const NFT_TOKEN_GROUPS_ORDER = [ 'ERC-721', 'ERC-1155', 'ERC-404' ] as const;

export const sortTokenGroups = (groupA: TokenGroup, groupB: TokenGroup) => {
  const additionalTypeIds = config.chain.additionalTokenTypes.map((item) => item.id);

  const tokenGroupsOrder = [
    'ERC-20',
    ...additionalTypeIds,
    ...NFT_TOKEN_GROUPS_ORDER,
  ] as const;

  const orderA = tokenGroupsOrder.indexOf(groupA[0] as (typeof tokenGroupsOrder)[number]);
  const orderB = tokenGroupsOrder.indexOf(groupB[0] as (typeof tokenGroupsOrder)[number]);

  const normalizedA = orderA === -1 ? 999 : orderA;
  const normalizedB = orderB === -1 ? 999 : orderB;

  if (normalizedA !== normalizedB) {
    return normalizedA > normalizedB ? 1 : -1;
  }

  return groupA[0].localeCompare(groupB[0]);
};

const sortErc1155or404Tokens = (sort: Sort) => (dataA: AddressTokenBalance, dataB: AddressTokenBalance) => {
  if (dataA.value === dataB.value) {
    return 0;
  }
  if (sort === 'desc') {
    return Number(dataA.value) > Number(dataB.value) ? -1 : 1;
  }

  return Number(dataA.value) > Number(dataB.value) ? 1 : -1;
};

const sortErc20Tokens = (sort: Sort) => (dataA: TokenEnhancedData, dataB: TokenEnhancedData) => {
  if (!dataA.usd && !dataB.usd) {
    return 0;
  }

  // keep tokens without usd value in the end of the group
  if (!dataB.usd) {
    return -1;
  }
  if (!dataA.usd) {
    return 0;
  }

  if (sort === 'desc') {
    return dataA.usd.gt(dataB.usd) ? -1 : 1;
  }

  return dataA.usd.gt(dataB.usd) ? 1 : -1;
};

const sortErc721Tokens = () => () => 0;

export const sortingFns = {
  'ERC-20': sortErc20Tokens,
  'ERC-721': sortErc721Tokens,
  'ERC-1155': sortErc1155or404Tokens,
  'ERC-404': sortErc1155or404Tokens,
};

export const getSortingFn = (typeId: string) => {
  return sortingFns[typeId as keyof typeof sortingFns] || sortErc20Tokens;
};

export const filterTokens = (searchTerm: string) => ({ token }: AddressTokenBalance) => {
  if (!token.name) {
    return !searchTerm ? true : token.address_hash.toLowerCase().includes(searchTerm);
  }

  return token.name?.toLowerCase().includes(searchTerm);
};

export const calculateUsdValue = (data: AddressTokenBalance): TokenEnhancedData => {
  const isFungibleToken = isFungibleTokenType(data.token.type);

  if (!isFungibleToken) {
    return data;
  }

  const exchangeRate = data.token.exchange_rate;
  if (!exchangeRate) {
    return data;
  }

  const decimals = Number(data.token.decimals || '18');
  return {
    ...data,
    usd: BigNumber(data.value).div(BigNumber(10 ** decimals)).multipliedBy(BigNumber(exchangeRate)),
  };
};

export interface TokensTotalInfo {
  usd: BigNumber;
  num: number;
  isOverflow: boolean;
}

export const getTokensTotalInfo = (data: TokenSelectData): TokensTotalInfo => {
  const usd = Object.values(data)
    .map(({ items }) => items.filter((item) => !isNativeToken(item)).reduce(usdValueReducer, ZERO))
    .reduce(sumBnReducer, ZERO);

  const num = Object.values(data)
    .map(({ items }) => items.length)
    .reduce((result, item) => result + item, 0);

  const isOverflow = Object.values(data).some(({ isOverflow }) => isOverflow);

  return { usd, num, isOverflow };
};

const usdValueReducer = (result: BigNumber, item: TokenEnhancedData) => !item.usd ? result : result.plus(BigNumber(item.usd));
