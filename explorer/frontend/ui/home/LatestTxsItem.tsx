import {
  Box,
  Flex,
  HStack,
  Text,
  Grid,
} from '@chakra-ui/react';
import React from 'react';

import type { Transaction } from 'types/api/transaction';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import EntityTag from 'ui/shared/EntityTags/EntityTag';
import TxStatus from 'ui/shared/statusTag/TxStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import TxFee from 'ui/shared/tx/TxFee';
import TxWatchListTags from 'ui/shared/tx/TxWatchListTags';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import TxAdditionalInfo from 'ui/txs/TxAdditionalInfo';
import TxType from 'ui/txs/TxType';

type Props = {
  tx: Transaction;
  isLoading?: boolean;
};

const LatestTxsItem = ({ tx, isLoading }: Props) => {
  const dataTo = tx.to ? tx.to : tx.created_contract;
  const columnNum = config.UI.views.tx.hiddenFields?.value && config.UI.views.tx.hiddenFields?.tx_fee ? 2 : 3;

  const protocolTag = tx.to?.metadata?.tags?.find(tag => tag.tagType === 'protocol');

  const tagsCount = [
    1, // tx type
    1, // tx status
    ...(tx.from?.watchlist_names || []),
    ...(tx.to?.watchlist_names || []),
    protocolTag,
  ].filter(Boolean).length;

  return (
    <Grid
      gridTemplateColumns={{
        lg: columnNum === 2 ? '3fr minmax(auto, 200px)' : '3fr minmax(auto, 200px) 170px',
        xl: columnNum === 2 ? '3fr minmax(auto, 270px)' : '3fr minmax(auto, 300px) 170px',
      }}
      gridGap={ 3 }
      width="100%"
      minW={ columnNum === 2 ? '700px' : '750px' }
      borderBottom="1px solid"
      borderColor="border.divider"
      p={ 4 }
      display={{ base: 'none', lg: 'grid' }}
    >
      <Flex overflow="hidden" w="100%">
        <TxAdditionalInfo tx={ tx } isLoading={ isLoading } my="3px"/>
        <Box ml={ 3 } w="calc(100% - 40px)">
          <HStack flexWrap={ tagsCount <= 3 ? 'nowrap' : 'wrap' } my="3px">
            <TxType types={ tx.transaction_types } isLoading={ isLoading }/>
            { tx.status !== 'ok' && <TxStatus status={ tx.status } errorText={ tx.status === 'error' ? tx.result : undefined } isLoading={ isLoading }/> }
            <TxWatchListTags tx={ tx } isLoading={ isLoading }/>
            { protocolTag && <EntityTag data={ protocolTag } isLoading={ isLoading } minW="0" noColors/> }
          </HStack>
          <Flex
            alignItems="center"
            mt="7px"
            mb="3px"
          >
            <TxEntity
              isLoading={ isLoading }
              hash={ tx.hash }
              fontWeight="700"
            />
            <TimeWithTooltip
              timestamp={ tx.timestamp }
              enableIncrement
              timeFormat="relative"
              isLoading={ isLoading }
              color="text.secondary"
              flexShrink={ 0 }
              ml={ 2 }
            />
          </Flex>
        </Box>
      </Flex>
      <AddressFromTo
        from={ tx.from }
        to={ dataTo }
        isLoading={ isLoading }
        mode="compact"
      />
      { !(config.UI.views.tx.hiddenFields?.value && config.UI.views.tx.hiddenFields?.tx_fee) ? (
        <Flex flexDir="column" rowGap={ 3 }>
          { !config.UI.views.tx.hiddenFields?.value && (
            <Skeleton loading={ isLoading }>
              <Text as="span" whiteSpace="pre">Value </Text>
              <NativeCoinValue
                amount={ tx.value }
                accuracy={ 5 }
                loading={ isLoading }
                color="text.secondary"
              />
            </Skeleton>
          ) }
          { !config.UI.views.tx.hiddenFields?.tx_fee && (
            <Skeleton loading={ isLoading } display="flex" whiteSpace="pre">
              <Text as="span">Fee </Text>
              <TxFee tx={ tx } accuracy={ 5 } color="text.secondary" noUsd/>
            </Skeleton>
          ) }
        </Flex>
      ) : null }
    </Grid>
  );
};

export default React.memo(LatestTxsItem);
