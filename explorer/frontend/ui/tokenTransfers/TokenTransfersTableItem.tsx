import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { ClusterChainConfig } from 'types/multichain';

import { isConfidentialTokenType, NFT_TOKEN_TYPE_IDS } from 'lib/token/tokenTypes';
import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import ConfidentialTokenValue from 'ui/shared/value/ConfidentialTokenValue';
import TokenValue from 'ui/shared/value/TokenValue';

type Props = {
  item: TokenTransfer;
  isLoading?: boolean;
  chainData?: ClusterChainConfig;
};

const TokenTransferTableItem = ({ item, isLoading, chainData }: Props) => {
  const isConfidential = item.token ? isConfidentialTokenType(item.token.type) : false;

  const renderValue = () => {
    if (item.token && item.total && 'value' in item.total && item.total.value !== null) {
      return (
        <TokenValue
          amount={ item.total.value }
          token={ item.token }
          decimals={ item.total.decimals || '0' }
          layout="vertical"
          loading={ isLoading }
        />
      );
    }

    if (isConfidential && item.token) {
      return (
        <ConfidentialTokenValue
          token={ item.token }
          loading={ isLoading }
        />
      );
    }

    return <Skeleton loading={ isLoading }>-</Skeleton>;
  };

  return (
    <TableRow>
      { chainData && (
        <TableCell>
          <ChainIcon data={ chainData } isLoading={ isLoading }/>
        </TableCell>
      ) }
      <TableCell>
        { item.transaction_hash ? (
          <TxEntity
            hash={ item.transaction_hash }
            isLoading={ isLoading }
            fontWeight={ 600 }
            noIcon
            truncation="constant_long"
          />
        ) : (
          <Skeleton loading={ isLoading }>-</Skeleton>
        ) }
        <TimeWithTooltip
          timestamp={ item.timestamp }
          enableIncrement
          isLoading={ isLoading }
          color="text.secondary"
          fontWeight="400"
          display="inline-block"
        />
      </TableCell>
      <TableCell maxW="120px">
        { item.method && <Badge loading={ isLoading }>{ item.method }</Badge> }
      </TableCell>
      <TableCell>
        <BlockEntity number={ item.block_number } isLoading={ isLoading } noIcon/>
      </TableCell>
      <TableCell>
        <AddressFromTo
          maxW={{ lg: '220px', xl: '320px' }}
          from={ item.from }
          to={ item.to }
          isLoading={ isLoading }
          mode={{ lg: 'compact', xl: 'long' }}
        />
      </TableCell>
      <TableCell>
        { item.total && 'token_id' in item.total && item.token && (NFT_TOKEN_TYPE_IDS.includes(item.token.type)) && item.total.token_id !== null ? (
          <NftEntity
            hash={ item.token.address_hash }
            id={ item.total.token_id }
            instance={ item.total.token_instance }
            isLoading={ isLoading }
            maxW="140px"
          />
        ) : <Skeleton loading={ isLoading }>-</Skeleton> }
      </TableCell>
      <TableCell isNumeric verticalAlign="top">
        { renderValue() }
      </TableCell>
    </TableRow>
  );
};

export default React.memo(TokenTransferTableItem);
