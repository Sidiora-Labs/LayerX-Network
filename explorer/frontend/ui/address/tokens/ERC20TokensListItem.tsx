import { Flex, HStack } from '@chakra-ui/react';
import { BigNumber } from 'bignumber.js';
import React from 'react';

import type { AddressTokensErc20Item } from './types';

import config from 'configs/app';
import multichainConfig from 'configs/multichain';
import { getTokenTypeName, isConfidentialTokenType } from 'lib/token/tokenTypes';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tag } from 'toolkit/chakra/tag';
import AddressAddToWallet from 'ui/shared/address/AddressAddToWallet';
import NativeTokenTag from 'ui/shared/celo/NativeTokenTag';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import calculateUsdValue from 'ui/shared/value/calculateUsdValue';
import ConfidentialValue from 'ui/shared/value/ConfidentialValue';
import SimpleValue from 'ui/shared/value/SimpleValue';
import { DEFAULT_ACCURACY_USD } from 'ui/shared/value/utils';

type Props = AddressTokensErc20Item & { isLoading: boolean; hasAdditionalTokenTypes?: boolean };

const ERC20TokensListItem = ({
  token,
  value,
  isLoading,
  chain_values: chainValues,
  hasAdditionalTokenTypes,
}: Props) => {
  const {
    valueBn: tokenQuantity,
    usdBn: tokenValue,
  } = calculateUsdValue({ amount: value, exchangeRate: token.exchange_rate, decimals: token.decimals });

  const isNativeToken = config.UI.views.address.nativeTokenAddress &&
    token.address_hash.toLowerCase() === config.UI.views.address.nativeTokenAddress.toLowerCase();

  const chainInfo = React.useMemo(() => {
    if (!chainValues) {
      return;
    }

    const chainId = Object.keys(chainValues)[0];
    const chain = multichainConfig()?.chains.find((chain) => chain.id === chainId);
    return chain;
  }, [ chainValues ]);

  return (
    <ListItemMobile rowGap={ 2 }>
      <Flex alignItems="center" width="100%" columnGap={ 2 }>
        <TokenEntity
          token={ token }
          chain={ chainInfo }
          isLoading={ isLoading }
          noCopy
          jointSymbol
          fontWeight="700"
          width="auto"
        />
        { isNativeToken && <NativeTokenTag/> }
        { hasAdditionalTokenTypes && <Tag loading={ isLoading }>{ getTokenTypeName(token.type) }</Tag> }
      </Flex>
      <Flex alignItems="center" pl={ 8 }>
        <AddressEntity
          address={{ hash: token.address_hash }}
          isLoading={ isLoading }
          truncation="constant"
          noIcon
        />
        <AddressAddToWallet token={ token } ml={ 2 } isLoading={ isLoading }/>
      </Flex>
      { token.exchange_rate !== undefined && token.exchange_rate !== null && (
        <HStack gap={ 3 }>
          <Skeleton loading={ isLoading } fontSize="sm" fontWeight={ 500 }>Price</Skeleton>
          <SimpleValue
            value={ BigNumber(token.exchange_rate) }
            loading={ isLoading }
            prefix="$"
            fontSize="sm"
            color="text.secondary"
          />
        </HStack>
      ) }
      <HStack gap={ 3 } alignItems="baseline">
        <Skeleton loading={ isLoading } fontSize="sm" fontWeight={ 500 }>Quantity</Skeleton>
        { isConfidentialTokenType(token.type) ? (
          <ConfidentialValue loading={ isLoading }/>
        ) : (
          <SimpleValue
            value={ tokenQuantity }
            loading={ isLoading }
            fontSize="sm"
            color="text.secondary"
          />
        ) }
      </HStack>
      { isConfidentialTokenType(token.type) && (
        <HStack gap={ 3 } alignItems="baseline">
          <Skeleton loading={ isLoading } fontSize="sm" fontWeight={ 500 }>Value</Skeleton>
          <ConfidentialValue loading={ isLoading }/>
        </HStack>
      ) }
      { !isConfidentialTokenType(token.type) && token.exchange_rate && (
        <HStack gap={ 3 } alignItems="baseline">
          <Skeleton loading={ isLoading } fontSize="sm" fontWeight={ 500 }>Value</Skeleton>
          <SimpleValue
            value={ tokenValue }
            prefix="$"
            loading={ isLoading }
            accuracy={ DEFAULT_ACCURACY_USD }
            fontSize="sm"
            color="text.secondary"
          />
        </HStack>
      ) }
    </ListItemMobile>
  );
};

export default ERC20TokensListItem;
