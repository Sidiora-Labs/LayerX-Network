import { chakra, Flex } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import { route } from 'nextjs/routes';

import config from 'configs/app';
import multichainConfig from 'configs/multichain';
import { isConfidentialTokenType, isFungibleTokenType } from 'lib/token/tokenTypes';
import { Link } from 'toolkit/chakra/link';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import NativeTokenTag from 'ui/shared/celo/NativeTokenTag';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import calculateUsdValue from 'ui/shared/value/calculateUsdValue';

import type { TokenEnhancedData } from '../utils/tokenUtils';

interface Props {
  data: TokenEnhancedData;
}

const TokenSelectItem = ({ data }: Props) => {

  const isNativeToken = config.UI.views.address.nativeTokenAddress &&
    data.token.address_hash.toLowerCase() === config.UI.views.address.nativeTokenAddress.toLowerCase();

  const chain = React.useMemo(() => {
    if (!data.chain_values) {
      return;
    }

    const chainId = Object.keys(data.chain_values)[0];
    const chain = multichainConfig()?.chains.find((chain) => chain.id === chainId);
    return chain;
  }, [ data.chain_values ]);

  const secondRow = (() => {
    if (isConfidentialTokenType(data.token.type)) {
      const text = `••••• ${ data.token.symbol || '' }`;

      return (
        <>
          <TruncatedText text={ text }/>
          { data.token.exchange_rate && <chakra.span ml={ 2 }>@{ Number(data.token.exchange_rate).toLocaleString() }</chakra.span> }
        </>
      );
    }

    const isFungibleToken = isFungibleTokenType(data.token.type);

    if (isFungibleToken) {
      const tokenDecimals = Number(data.token.decimals ?? 18);
      const text = `${ BigNumber(data.value).dividedBy(10 ** tokenDecimals).toFormat() } ${ data.token.symbol || '' }`;

      return (
        <>
          <TruncatedText text={ text }/>
          { data.token.exchange_rate && <chakra.span ml={ 2 }>@{ Number(data.token.exchange_rate).toLocaleString() }</chakra.span> }
        </>
      );
    }

    switch (data.token.type) {
      case 'ERC-721': {
        const text = `${ BigNumber(data.value).toFormat() } ${ data.token.symbol || '' }`;
        return <TruncatedText text={ text }/>;
      }
      case 'ERC-1155': {
        return (
          <>
            <chakra.span textOverflow="ellipsis" overflow="hidden" mr={ 6 }>
              #{ data.token_id || 0 }
            </chakra.span>
            <span>
              { BigNumber(data.value).toFormat() }
            </span>
          </>
        );
      }
      case 'ERC-404': {
        return (
          <>
            { data.token_id !== null && (
              <chakra.span textOverflow="ellipsis" overflow="hidden" mr={ 6 }>
                #{ data.token_id || 0 }
              </chakra.span>
            ) }
            { data.value !== null && (
              <span>
                { data.token.decimals ?
                  calculateUsdValue({ amount: data.value, decimals: data.token.decimals }).valueStr :
                  BigNumber(data.value).toFormat()
                }
              </span>
            ) }
          </>
        );
      }
    }
  })();

  const url = route({ pathname: '/token/[hash]', query: { hash: data.token.address_hash } }, { chain });

  return (
    <Link
      px={ 1 }
      py="10px"
      display="flex"
      flexDir="column"
      rowGap={ 2 }
      borderColor="border.divider"
      borderBottomWidth="1px"
      _hover={{
        bgColor: { _light: 'blue.50', _dark: 'gray.800' },
      }}
      color="unset"
      fontSize="sm"
      href={ url }
    >
      <Flex alignItems="center" w="100%">
        <TokenEntity
          token={ data.token }
          chain={ chain }
          noSymbol
          noCopy
          noLink
          fontWeight={ 700 }
          width="auto"
          mr={ 2 }
        />
        { isNativeToken && <NativeTokenTag mr={ 2 }/> }
        { data.usd && (
          <TruncatedText
            text={ `$${ data.usd.toFormat(2) }` }
            fontWeight={ 700 }
            minW="120px"
            ml="auto"
            textAlign="right"
            color={ isNativeToken ? 'text.secondary' : undefined }
          />
        ) }
      </Flex>
      <Flex alignItems="center" justifyContent="space-between" w="100%" whiteSpace="nowrap" color={ isNativeToken ? 'text.secondary' : undefined }>
        { secondRow }
      </Flex>
    </Link>
  );
};

export default React.memo(TokenSelectItem);
