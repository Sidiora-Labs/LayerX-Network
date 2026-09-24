import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { AdvancedFilterResponseItem } from 'types/api/advancedFilter';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { isConfidentialTokenType } from 'lib/token/tokenTypes';
import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';
import type { ColumnsIds } from 'ui/advancedFilter/constants';
import AddressFromToIcon from 'ui/shared/address/AddressFromToIcon';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import AssetValue from 'ui/shared/value/AssetValue';
import ConfidentialValue from 'ui/shared/value/ConfidentialValue';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

import { getAdvancedFilterTypes } from './constants';

type Props = {
  item: AdvancedFilterResponseItem;
  column: ColumnsIds;
  isLoading?: boolean;
  chainConfig?: ClusterChainConfig['app_config'];
};

const ItemByColumn = ({ item, column, isLoading, chainConfig }: Props) => {
  switch (column) {
    case 'tx_hash':
      return <TxEntity truncation="constant" hash={ item.hash } isLoading={ isLoading } noIcon fontWeight={ 700 }/>;
    case 'type': {
      const type = getAdvancedFilterTypes(chainConfig).find(t => t.id === item.type);
      if (!type) {
        return null;
      }
      return <Badge loading={ isLoading }>{ type.name }</Badge>;
    }
    case 'method':
      return item.method ? <Badge loading={ isLoading } truncated>{ item.method }</Badge> : null;
    case 'age':
      return <TimeWithTooltip timestamp={ item.timestamp } isLoading={ isLoading } color="text.secondary" fontWeight={ 400 }/>;
    case 'from':
      return (
        <Flex w="100%">
          <AddressEntity address={ item.from } truncation="constant" isLoading={ isLoading }/>
        </Flex>
      );
    case 'to': {
      const address = item.to ? item.to : item.created_contract;
      if (!address) {
        return null;
      }
      return (
        <Flex w="100%">
          <AddressEntity address={ address } truncation="constant" isLoading={ isLoading }/>
        </Flex>
      );
    }
    case 'or_and':
      return (
        <AddressFromToIcon
          isLoading={ isLoading }
          type="unspecified"
        />
      );
    case 'amount': {
      if (item.token?.type === 'ERC-721') {
        return <Skeleton loading={ isLoading }>1</Skeleton>;
      }
      if (item.token && isConfidentialTokenType(item.token.type)) {
        return <ConfidentialValue loading={ isLoading }/>;
      }
      if (item.total) {
        return (
          <AssetValue
            amount={ item.total?.value }
            decimals={ item.total.decimals }
            loading={ isLoading }
          />
        );
      }
      if (item.value) {
        return (
          <NativeCoinValue
            amount={ item.value }
            noSymbol
            loading={ isLoading }
          />
        );
      }
      return null;
    }
    case 'asset':
      return item.token ?
        <TokenEntity token={ item.token } isLoading={ isLoading } fontWeight={ 700 } onlySymbol noCopy/> :
        <Skeleton loading={ isLoading } fontWeight={ 700 }>{ config.chain.currency.symbol }</Skeleton>;
    case 'fee':
      return (
        <NativeCoinValue
          amount={ item.fee }
          noSymbol
          loading={ isLoading }
        />
      );
    default:
      return null;
  }
};

export default ItemByColumn;
