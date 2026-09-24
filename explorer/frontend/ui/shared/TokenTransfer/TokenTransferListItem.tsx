import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { ClusterChainConfig } from 'types/multichain';

import { getTokenTypeName, isConfidentialTokenType } from 'lib/token/tokenTypes';
import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import { getTokenTransferTypeText } from 'ui/shared/TokenTransfer/helpers';
import AssetValue from 'ui/shared/value/AssetValue';
import ConfidentialValue from 'ui/shared/value/ConfidentialValue';
import TxAdditionalInfo from 'ui/txs/TxAdditionalInfo';

type Props = TokenTransfer & {
  baseAddress?: string;
  showTxInfo?: boolean;
  enableTimeIncrement?: boolean;
  isLoading?: boolean;
  chainData?: ClusterChainConfig;
};

const TokenTransferListItem = ({
  token,
  total,
  transaction_hash: txHash,
  from,
  to,
  baseAddress,
  showTxInfo,
  type,
  timestamp,
  enableTimeIncrement,
  isLoading,
  chainData,
}: Props) => {
  return (
    <ListItemMobile rowGap={ 3 }>
      <Flex w="100%" justifyContent="space-between">
        <Flex flexWrap="wrap" rowGap={ 1 } mr={ showTxInfo && txHash ? 2 : 0 } columnGap={ 2 } overflow="hidden">
          { token && (
            <>
              <TokenEntity
                token={ token }
                isLoading={ isLoading }
                noSymbol
                noCopy
                w="auto"
              />
              <Badge flexShrink={ 0 } loading={ isLoading }>{ getTokenTypeName(token.type, chainData?.app_config) }</Badge>
            </>
          ) }
          <Badge colorPalette="orange" loading={ isLoading }>{ getTokenTransferTypeText(type) }</Badge>
        </Flex>
        { showTxInfo && txHash && (
          <TxAdditionalInfo hash={ txHash } isMobile isLoading={ isLoading }/>
        ) }
      </Flex>
      { total && 'token_id' in total && total.token_id !== null && token && (
        <NftEntity hash={ token.address_hash } id={ total.token_id } instance={ total.token_instance } isLoading={ isLoading }/>
      ) }
      { showTxInfo && (
        <Flex justifyContent="space-between" alignItems="center" lineHeight="24px" width="100%">
          { txHash && (
            <TxEntity
              isLoading={ isLoading }
              hash={ txHash }
              truncation="constant_long"
              fontWeight="700"
              chain={ chainData }
            />
          ) }
          <TimeWithTooltip
            timestamp={ timestamp }
            enableIncrement={ enableTimeIncrement }
            isLoading={ isLoading }
            color="text.secondary"
            fontWeight="400"
            fontSize="sm"
          />
        </Flex>
      ) }
      <AddressFromTo
        from={ from }
        to={ to }
        current={ baseAddress }
        isLoading={ isLoading }
        w="100%"
      />
      { total && 'value' in total && total.value !== null && (
        <Flex columnGap={ 2 } w="100%">
          <Skeleton loading={ isLoading } fontWeight={ 500 } flexShrink={ 0 }>Value</Skeleton>
          <AssetValue
            amount={ total && 'value' in total && total.value !== null ? total.value : null }
            decimals={ total && 'decimals' in total ? total.decimals || '0' : '0' }
            exchangeRate={ token?.exchange_rate }
            loading={ isLoading }
            color="text.secondary"
          />
        </Flex>
      ) }

      { token && isConfidentialTokenType(token.type) && (!total || !('value' in total) || total.value === null) && (
        <Flex columnGap={ 2 } w="100%">
          <Skeleton loading={ isLoading } fontWeight={ 500 } flexShrink={ 0 }>Value</Skeleton>
          <ConfidentialValue loading={ isLoading } color="text.secondary"/>
        </Flex>
      ) }
    </ListItemMobile>
  );
};

export default React.memo(TokenTransferListItem);
