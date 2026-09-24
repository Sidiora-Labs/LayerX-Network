import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import BatchEntityL2 from 'ui/shared/entities/block/BatchEntityL2';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

type Props = {
  number: number;
  timestamp: string | null;
  txCount: number;
  status?: React.ReactNode;
  isLoading: boolean;
  animation?: string;
};

const LatestBatchItem = ({ number, timestamp, txCount, status, isLoading, animation }: Props) => {
  return (
    <Box
      animation={ animation }
      borderRadius="md"
      border="1px solid"
      borderColor="border.divider"
      p={ 3 }
    >
      <Flex alignItems="center" overflow="hidden" w="100%" mb={ 3 }>
        <BatchEntityL2
          isLoading={ isLoading }
          number={ number }
          tailLength={ 2 }
          textStyle="md"
          fontWeight={ 500 }
          mr="auto"
        />
        <TimeWithTooltip
          timestamp={ timestamp }
          enableIncrement={ !isLoading }
          timeFormat="relative"
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
          textStyle="sm"
          flexShrink={ 0 }
          ml={ 2 }
        />
      </Flex>
      <Flex alignItems="center" justifyContent="space-between" w="100%" flexWrap="wrap" textStyle="sm">
        <Flex alignItems="center">
          <Skeleton loading={ isLoading } mr={ 2 }>Txn</Skeleton>
          <Link
            href={ route({ pathname: '/batches/[number]', query: { number: number.toString(), tab: 'txs' } }) }
            loading={ isLoading }
          >
            { txCount }
          </Link>
        </Flex>
        { status }
      </Flex>
    </Box>
  );
};

export default LatestBatchItem;
