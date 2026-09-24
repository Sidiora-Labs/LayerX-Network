import { Flex, VStack } from '@chakra-ui/react';
import React from 'react';

import type { NovesDescribeTxsResponse } from 'types/api/noves';
import type { Transaction } from 'types/api/transaction';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { Badge } from 'toolkit/chakra/badge';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import BlockPendingUpdateHint from 'ui/shared/block/BlockPendingUpdateHint';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import EntityTag from 'ui/shared/EntityTags/EntityTag';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import TxStatus from 'ui/shared/statusTag/TxStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import TxFee from 'ui/shared/tx/TxFee';
import TxWatchListTags from 'ui/shared/tx/TxWatchListTags';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import TxAdditionalInfo from 'ui/txs/TxAdditionalInfo';

import TxTranslationType from './TxTranslationType';
import TxType from './TxType';

type Props = {
  tx: Transaction;
  showBlockInfo: boolean;
  currentAddress?: string;
  enableTimeIncrement?: boolean;
  isLoading?: boolean;
  animation?: string;
  chainData?: ClusterChainConfig;
  translationIsLoading?: boolean;
  translationData?: NovesDescribeTxsResponse;
  isMobile?: boolean;
};

const TxsTableItem = ({
  tx,
  showBlockInfo,
  currentAddress,
  enableTimeIncrement,
  isLoading,
  animation,
  chainData,
  translationIsLoading,
  translationData,
  isMobile,
}: Props) => {
  const dataTo = tx.to ? tx.to : tx.created_contract;

  const protocolTag = tx.to?.hash !== currentAddress && tx.to?.metadata?.tags?.find(tag => tag.tagType === 'protocol');

  return (
    <TableRow key={ tx.hash } animation={ animation }>
      <TableCell textAlign="center">
        <TxAdditionalInfo tx={ tx } isMobile={ isMobile } isLoading={ isLoading }/>
      </TableCell>
      { chainData && (
        <TableCell>
          <ChainIcon data={ chainData } isLoading={ isLoading } my="2px"/>
        </TableCell>
      ) }
      <TableCell pr={ 4 }>
        <VStack alignItems="start" lineHeight="24px">
          <TxEntity
            hash={ tx.hash }
            isLoading={ isLoading }
            fontWeight="bold"
            noIcon
            maxW="100%"
            truncation="constant"
          />
          <TimeWithTooltip
            timestamp={ tx.timestamp }
            enableIncrement={ enableTimeIncrement }
            isLoading={ isLoading }
            color="text.secondary"
          />
        </VStack>
      </TableCell>
      <TableCell>
        <VStack alignItems="stretch">
          { translationIsLoading || translationData ? (
            <TxTranslationType
              txTypes={ tx.transaction_types }
              isLoading={ isLoading || translationIsLoading }
              type={ translationData?.type }
            />
          ) :
            <TxType types={ tx.transaction_types } isLoading={ isLoading }/>
          }
          { tx.status !== 'ok' && <TxStatus status={ tx.status } errorText={ tx.status === 'error' ? tx.result : undefined } isLoading={ isLoading }/> }
          <TxWatchListTags tx={ tx } isLoading={ isLoading }/>
        </VStack>
      </TableCell>
      <TableCell whiteSpace="nowrap">
        <VStack alignItems="flex-start">
          { tx.method && (
            <Badge colorPalette={ tx.method === 'Multicall' ? 'teal' : 'gray' } loading={ isLoading } truncated>
              <span>{ tx.method }</span>
            </Badge>
          ) }
          { protocolTag && <EntityTag data={ protocolTag } isLoading={ isLoading } maxW="100%" noColors/> }
        </VStack>
      </TableCell>
      { showBlockInfo && (
        <TableCell>
          <Flex alignItems="center" gap={ 2 }>
            { tx.block_number && (
              <BlockEntity
                isLoading={ isLoading }
                number={ tx.block_number }
                noIcon
                textStyle="sm"
                fontWeight={ 500 }
              />
            ) }
            { tx.is_pending_update && <BlockPendingUpdateHint view="tx"/> }
          </Flex>
        </TableCell>
      ) }
      <TableCell>
        <AddressFromTo
          from={ tx.from }
          to={ dataTo }
          current={ currentAddress }
          isLoading={ isLoading }
          mt="2px"
          mode="compact"
        />
      </TableCell>
      { !config.UI.views.tx.hiddenFields?.value && (
        <TableCell isNumeric>
          <NativeCoinValue
            amount={ tx.value }
            noSymbol
            loading={ isLoading }
            exchangeRate={ tx.exchange_rate }
            historicalExchangeRate={ tx.historic_exchange_rate }
            layout="vertical"
            rowGap={ 3 }
          />
        </TableCell>
      ) }
      { !config.UI.views.tx.hiddenFields?.tx_fee && (
        <TableCell isNumeric maxW="220px">
          <TxFee
            tx={ tx }
            accuracy={ 8 }
            loading={ isLoading }
            noSymbol={ !(tx.celo || tx.stability_fee) }
            layout="vertical"
            rowGap={ 3 }
          />
        </TableCell>
      ) }
    </TableRow>
  );
};

export default React.memo(TxsTableItem);
