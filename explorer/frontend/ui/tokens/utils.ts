import type { TokensSortingValue } from 'types/api/tokens';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import getFilterValuesFromQuery from 'lib/getFilterValuesFromQuery';
import { getTokenTypes } from 'lib/token/tokenTypes';
import type { SelectOption } from 'toolkit/chakra/select';

export const SORT_OPTIONS: Array<SelectOption<TokensSortingValue>> = [
  { label: 'Default', value: 'default' },
  { label: 'Price ascending', value: 'fiat_value-asc' },
  { label: 'Price descending', value: 'fiat_value-desc' },
  { label: 'Holders ascending', value: 'holders_count-asc' },
  { label: 'Holders descending', value: 'holders_count-desc' },
  { label: 'On-chain market cap ascending', value: 'circulating_market_cap-asc' },
  { label: 'On-chain market cap descending', value: 'circulating_market_cap-desc' },
];

export const getTokenFilterValue = (
  value: string | Array<string> | undefined,
  chainConfig?: Array<ClusterChainConfig['app_config']> | ClusterChainConfig['app_config'],
) => {
  const tokenTypes = getTokenTypes(false, chainConfig);
  return getFilterValuesFromQuery(Object.keys(tokenTypes), value);
};

const bridgedTokensChainIds = (() => {
  const feature = config.features.bridgedTokens;
  if (!feature.isEnabled) {
    return [];
  }

  return feature.chains.map(chain => chain.id);
})();
export const getBridgedChainsFilterValue = (getFilterValuesFromQuery<string>).bind(null, bridgedTokensChainIds);
