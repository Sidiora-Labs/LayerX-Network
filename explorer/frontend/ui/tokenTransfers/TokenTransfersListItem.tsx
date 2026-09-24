import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { ClusterChainConfig } from 'types/multichain';

import { hasTokenTransferValue, isConfidentialTokenType, NFT_TOKEN_TYPE_IDS } from 'lib/token/tokenTypes';
import { Badge } from 'toolkit/chakra/badge';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import ConfidentialTokenValue from 'ui/shared/value/ConfidentialTokenValue';
import TokenValue from 'ui/shared/value/TokenValue';

type Props = {
  item: TokenTransfer;
  isLoading: boolean;
  chainData?: ClusterChainConfig;
};

const TokenTransfersListItem = ({ item, isLoading, chainData }: Props) => {
  const isConfidential = item.token ? isConfidentialTokenType(item.token.type) : false;

  return (
    <ListItemMobileGrid.Container>
      { item.transaction_hash && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Txn hash</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <TxEntity hash={ item.transaction_hash } isLoading={ isLoading } truncation="constant_long" noIcon={ !chainData } chain={ chainData }/>
          </ListItemMobileGrid.Value>
        </>
      ) }

      <ListItemMobileGrid.Label isLoading={ isLoading }>Age</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <TimeWithTooltip
          timestamp={ item.timestamp }
          enableIncrement
          isLoading={ isLoading }
        />
      </ListItemMobileGrid.Value>

      { item.method && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Method</ListItemMobileGrid.Label><ListItemMobileGrid.Value>
            <Badge loading={ isLoading }>{ item.method }</Badge>
          </ListItemMobileGrid.Value>
        </>
      ) }

      <ListItemMobileGrid.Label isLoading={ isLoading }>Block</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <BlockEntity number={ item.block_number } isLoading={ isLoading } noIcon/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>From</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <AddressEntity address={ item.from } isLoading={ isLoading } truncation="constant"/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>To</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <AddressEntity address={ item.to } isLoading={ isLoading } truncation="constant"/>
      </ListItemMobileGrid.Value>

      { item.total && 'token_id' in item.total && item.token && (NFT_TOKEN_TYPE_IDS.includes(item.token.type)) && item.total.token_id !== null && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Token ID</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value overflow="hidden">
            <NftEntity
              hash={ item.token.address_hash }
              id={ item.total.token_id }
              instance={ item.total.token_instance }
              isLoading={ isLoading }
              noIcon
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      { item.token && item.total && 'value' in item.total && item.total.value !== null && (hasTokenTransferValue(item.token.type)) && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Amount</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <TokenValue
              amount={ item.total.value }
              token={ item.token }
              decimals={ item.total.decimals || '0' }
              loading={ isLoading }
            />
          </ListItemMobileGrid.Value>
        </>
      ) }

      { isConfidential && item.token && (!item.total || !('value' in item.total) || item.total.value === null) && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Amount</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <ConfidentialTokenValue
              token={ item.token }
              loading={ isLoading }
            />
          </ListItemMobileGrid.Value>
        </>
      ) }
    </ListItemMobileGrid.Container>
  );
};

export default TokenTransfersListItem;
