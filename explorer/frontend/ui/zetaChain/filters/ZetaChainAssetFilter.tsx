import { Flex, Text, Spinner } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import React from 'react';

import type { Token } from '@blockscout/zetachain-cctx-types';
import type { TokenInfo } from 'types/api/token';
import { ZETA_CHAIN_CCTX_COIN_TYPE_FILTER, type ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import useDebounce from 'lib/hooks/useDebounce';
import { PopoverCloseTriggerWrapper } from 'toolkit/chakra/popover';
import { Tag } from 'toolkit/chakra/tag';
import { ClearButton } from 'toolkit/components/buttons/ClearButton';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import * as TokenEntity from 'ui/shared/entities/token/TokenEntity';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';
import NativeTokenIcon from 'ui/shared/NativeTokenIcon';

const FILTER_PARAM_SYMBOL = 'token_symbol';
const FILTER_PARAM_COIN_TYPE = 'coin_type';

const getFilterParamsFromValue = (value: Value) => {
  if (value?.symbol === ZETA_NATIVE_TOKEN.symbol) {
    return {
      [ FILTER_PARAM_COIN_TYPE ]: [ ZETA_CHAIN_CCTX_COIN_TYPE_FILTER ],
      [ FILTER_PARAM_SYMBOL ]: [],
    };
  } else if (value?.symbol) {
    return {
      [ FILTER_PARAM_SYMBOL ]: [ value?.symbol ],
      [ FILTER_PARAM_COIN_TYPE ]: [],
    };
  }
  return { [ FILTER_PARAM_SYMBOL ]: [], [ FILTER_PARAM_COIN_TYPE ]: [] };
};

// ZETA native token constant
export const ZETA_NATIVE_TOKEN = {
  name: config.chain.currency.name,
  icon_url: '',
  symbol: config.chain.currency.symbol,
  address_hash: 'native',
  type: 'ERC-20',
} as TokenInfo;

// We can't implement multivalue search here, because we have different search params for native coin and for tokens.
type Value = TokenInfo | null;

type Props = {
  value: Value;
  handleFilterChange: (field: keyof ZetaChainCCTXFilterParams, val: Array<string>) => void;
  columnName: string;
  isLoading?: boolean;
};

const ZetaChainAssetFilter = ({ value = null, handleFilterChange }: Props) => {
  const [ currentValue, setCurrentValue ] = React.useState<Value>(value);
  const [ searchTerm, setSearchTerm ] = React.useState<string>('');
  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const onSearchChange = React.useCallback((value: string) => {
    setSearchTerm(value);
  }, []);

  const handleRemove = React.useCallback(() => {
    setCurrentValue(null);
  }, []);

  const tokensQuery = useApiQuery('zetachain:tokens', {
    queryParams: {},
    queryOptions: {
      refetchOnMount: false,
    },
  });

  // Filter tokens based on search term
  const filteredTokens = React.useMemo(() => {
    if (!tokensQuery.data?.tokens) return [];

    if (!debouncedSearchTerm) {
      return tokensQuery.data.tokens.filter(token => token.symbol !== 'UNKNOWN').slice(0, 5);
    }

    const searchLower = debouncedSearchTerm.toLowerCase();
    return tokensQuery.data.tokens.filter(token =>
      (token.symbol?.toLowerCase().includes(searchLower) ||
       token.name?.toLowerCase().includes(searchLower)),
    );
  }, [ tokensQuery.data?.tokens, debouncedSearchTerm ]);

  const onReset = React.useCallback(() => setCurrentValue(null), []);

  const onFilter = React.useCallback(() => {
    setSearchTerm('');
    const filterParams = getFilterParamsFromValue(currentValue);
    handleFilterChange(FILTER_PARAM_COIN_TYPE, filterParams[ FILTER_PARAM_COIN_TYPE ]);
    handleFilterChange(FILTER_PARAM_SYMBOL, filterParams[ FILTER_PARAM_SYMBOL ]);
    return;
  }, [ handleFilterChange, currentValue ]);

  const onTokenClick = React.useCallback((token: Token | TokenInfo) => () => {
    // Convert to TokenInfo for compatibility
    const tokenInfo: TokenInfo = 'zrc20_contract_address' in token ? {
      address_hash: token.zrc20_contract_address,
      symbol: token.symbol,
      name: token.name,
      decimals: token.decimals.toString(),
      total_supply: null,
      icon_url: token.icon_url ?? null,
      type: 'ERC-20', // Default type for ZetaChain tokens
      holders_count: null,
      exchange_rate: null,
      circulating_market_cap: null,
      reputation: null,
    } : token;
    setCurrentValue(tokenInfo);
    const filterParams = getFilterParamsFromValue(tokenInfo);
    handleFilterChange(FILTER_PARAM_COIN_TYPE, filterParams[ FILTER_PARAM_COIN_TYPE ]);
    handleFilterChange(FILTER_PARAM_SYMBOL, filterParams[ FILTER_PARAM_SYMBOL ]);
  }, [ handleFilterChange ]);

  return (
    <TableColumnFilter
      title="Asset"
      isFilled={ Boolean(currentValue) }
      isTouched={ !isEqual(currentValue, value) }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <FilterInput
        placeholder="Search by token name or symbol"
        initialValue={ searchTerm }
        onChange={ onSearchChange }
        loading={ tokensQuery.isLoading }
      />
      { !searchTerm && currentValue && (
        <Flex key={ currentValue.address_hash } alignItems="center">
          <Flex alignItems="center" gap={ 2 } flexGrow={ 1 }>
            { currentValue.symbol === 'ZETA' ? (
              <NativeTokenIcon boxSize={ 5 } mr={ 2 }/>
            ) : (
              <TokenEntity.Icon token={ currentValue }/>
            ) }
            <TokenEntity.Content token={ currentValue } onlySymbol/>
          </Flex>
          <ClearButton onClick={ handleRemove }/>
        </Flex>
      ) }
      { tokensQuery.isLoading && <Spinner display="block" mt={ 3 }/> }
      { tokensQuery.data && !searchTerm && (
        <>
          <Text color="text.secondary" fontWeight="600" mt={ 3 }>Popular</Text>
          <Flex rowGap={ 3 } flexWrap="wrap" gap={ 3 } mb={ 2 }>
            { [ ZETA_NATIVE_TOKEN, ...filteredTokens.map(token => ({
              address_hash: token.zrc20_contract_address,
              symbol: token.symbol,
              name: token.name,
              decimals: token.decimals.toString(),
              total_supply: null,
              icon_url: token.icon_url,
              type: 'ERC-20' as const,
              holders_count: null,
              exchange_rate: null,
              circulating_market_cap: null,
            } as TokenInfo)) ].map(token => (
              <PopoverCloseTriggerWrapper key={ token.address_hash }>
                <Tag
                  data-id={ token.address_hash }
                  onClick={ onTokenClick(token) }
                  variant="select"
                >
                  <Flex flexGrow={ 1 } alignItems="center">
                    { token.address_hash === ZETA_NATIVE_TOKEN.address_hash ? <NativeTokenIcon boxSize={ 5 } mr={ 2 }/> : <TokenEntity.Icon token={ token }/> }
                    { token.symbol || token.name || token.address_hash }
                  </Flex>
                </Tag>
              </PopoverCloseTriggerWrapper>
            )) }
          </Flex>
        </>
      ) }
      { searchTerm && tokensQuery.data && !filteredTokens.length && <Text>No tokens found</Text> }
      { searchTerm && tokensQuery.data && Boolean(filteredTokens.length) && (
        <Flex display="flex" flexDir="column" rowGap={ 3 } maxH="250px" overflowY="scroll" mt={ 3 } ml="-4px">
          { filteredTokens.map(token => (
            <PopoverCloseTriggerWrapper key={ token.zrc20_contract_address }>
              <Flex
                alignItems="center"
                p={ 2 }
                borderRadius="md"
                cursor="pointer"
                _hover={{ bg: 'gray.50' }}
                onClick={ onTokenClick(token) }
              >
                { /* FIXME: I'd use TokenEntity here, but it prevents onTokenClick callback from being called */ }
                <TokenEntity.Icon token={{
                  address_hash: token.zrc20_contract_address,
                  symbol: token.symbol,
                  name: token.name,
                  decimals: token.decimals.toString(),
                  total_supply: null,
                  icon_url: token.icon_url,
                  type: 'ERC-20',
                  holders_count: null,
                  exchange_rate: null,
                  circulating_market_cap: null,
                } as TokenInfo}/>
                { token.symbol || token.name || token.zrc20_contract_address }
              </Flex>
            </PopoverCloseTriggerWrapper>
          )) }
        </Flex>
      ) }
    </TableColumnFilter>
  );
};

export default ZetaChainAssetFilter;
