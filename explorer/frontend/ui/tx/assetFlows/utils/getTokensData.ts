import { groupBy, mapValues } from 'es-toolkit';

import type { NovesResponseData } from 'types/api/noves';
import type { TokenInfo } from 'types/api/token';

import { HEX_REGEXP } from 'toolkit/utils/regexp';

export interface NovesTokenInfo extends Pick<TokenInfo, 'name' | 'symbol'> {
  id?: string | undefined;
  address?: string;
}

export interface TokensData {
  nameList: Array<string>;
  symbolList: Array<string>;
  idList: Array<string>;
  byName: {
    [x: string]: NovesTokenInfo;
  };
  bySymbol: {
    [x: string]: NovesTokenInfo;
  };
}

export function getTokensData(data: NovesResponseData): TokensData {
  const sent = data.classificationData.sent || [];
  const received = data.classificationData.received || [];
  const approved = data.classificationData.approved ? [ data.classificationData.approved ] : [];

  const txItems = [ ...sent, ...received, ...approved ];

  // Extract all tokens data
  const tokens = txItems.map((item) => {
    const name = item.nft?.name || item.token?.name || null;
    const symbol = item.nft?.symbol || item.token?.symbol || null;
    const address = item.nft?.address || item.token?.address || '';

    const validTokenAddress = address ? HEX_REGEXP.test(address) : false;

    const token = {
      name: name,
      symbol: symbol?.toLowerCase() === name?.toLowerCase() ? null : symbol,
      address: validTokenAddress ? address : '',
      id: item.nft?.id || item.token?.id,
    };

    return token;
  });

  // Group tokens by property into arrays
  const tokensGroupByName = groupBy(tokens, (item) => item.name || 'null');
  const tokensGroupBySymbol = groupBy(tokens, (item) => item.symbol || 'null');
  const tokensGroupById = groupBy(tokens, (item) => item.id || 'null');

  // Map properties to an object and remove duplicates
  const mappedNames = mapValues(tokensGroupByName, (i) => {
    return i[0];
  });

  const mappedSymbols = mapValues(tokensGroupBySymbol, (i) => {
    return i[0];
  });

  const mappedIds = mapValues(tokensGroupById, (i) => {
    return i[0];
  });

  const filters = [ 'undefined', 'null' ];
  // Array of keys to match in string
  const nameList = Object.keys(mappedNames).filter(i => !filters.includes(i));
  const symbolList = Object.keys(mappedSymbols).filter(i => !filters.includes(i));
  const idList = Object.keys(mappedIds).filter(i => !filters.includes(i));

  return {
    nameList,
    symbolList,
    idList,
    byName: mappedNames,
    bySymbol: mappedSymbols,
  };
}
