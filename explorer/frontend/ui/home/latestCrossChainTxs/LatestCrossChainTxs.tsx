import { Box, Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import useApiQuery from 'lib/api/useApiQuery';
import useIsMobile from 'lib/hooks/useIsMobile';
import { INTERCHAIN_MESSAGE } from 'stubs/interchainIndexer';
import { generateListStub } from 'stubs/utils';
import { Link } from 'toolkit/chakra/link';
import { TableBody, TableRoot } from 'toolkit/chakra/table';
import TransactionsCrossChainListItem from 'ui/crossChain/txs/TransactionsCrossChainListItem';

import LatestCrossChainTxsItemDesktop from './LatestCrossChainTxsItemDesktop';

const LatestCrossChainTxs = () => {
  const isMobile = useIsMobile();
  const txsCount = isMobile ? 2 : 5;
  const { data, isPlaceholderData, isError } = useApiQuery('interchainIndexer:messages', {
    queryOptions: {
      placeholderData: generateListStub<'interchainIndexer:messages'>(
        INTERCHAIN_MESSAGE,
        txsCount,
        {
          next_page_params: undefined,
        },
      ),
    },
  });

  if (isError || !data) {
    return <Text mt={ 4 }>No data. Please reload the page.</Text>;
  }

  return (
    <>
      <Box mb={ 3 } hideFrom="lg" textStyle="sm">
        { data.items.slice(0, txsCount).map(((tx, index) => (
          <TransactionsCrossChainListItem
            key={ tx.message_id + (isPlaceholderData ? index : '') }
            data={ tx }
            isLoading={ isPlaceholderData }
            py={ 4 }
            textStyle="sm"
            rowGap="14px"
            _first={{
              borderTopWidth: '0',
              paddingTop: '4px',
            }}
          />
        ))) }
      </Box>
      <Box mb={ 3 } hideBelow="lg" textStyle="sm">
        <TableRoot minWidth="750px">
          <TableBody>
            { data.items.slice(0, txsCount).map(((tx, index) => (
              <LatestCrossChainTxsItemDesktop
                key={ tx.message_id + (isPlaceholderData ? index : '') }
                data={ tx }
                isLoading={ isPlaceholderData }
              />
            ))) }
          </TableBody>
        </TableRoot>
      </Box>
      <Flex justifyContent="center">
        <Link textStyle="sm" href={ route({ pathname: '/txs', query: { tab: 'txs_cross_chain' } }) }>View all transactions</Link>
      </Flex>
    </>
  );
};

export default React.memo(LatestCrossChainTxs);
