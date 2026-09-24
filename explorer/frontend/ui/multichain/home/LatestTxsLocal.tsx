import { Separator } from '@chakra-ui/react';
import React from 'react';

import type { PaginationParams } from 'ui/shared/pagination/types';

import { route } from 'nextjs/routes';

import getSocketUrl from 'lib/api/getSocketUrl';
import useApiQuery from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import { SocketProvider } from 'lib/socket/context';
import { TX } from 'stubs/tx';
import { Link } from 'toolkit/chakra/link';
import TxsContent from 'ui/txs/TxsContent';

const PAGINATION_PARAMS: PaginationParams = {
  page: 1,
  isVisible: false,
  isLoading: false,
  hasPages: false,
  hasNextPage: false,
  canGoBackwards: false,
  onNextPageClick: () => {},
  onPrevPageClick: () => {},
  resetPage: () => {},
};

const LatestTxsLocal = () => {
  const chain = useMultichainContext()?.chain;

  const query = useApiQuery('general:homepage_txs', {
    chain,
    queryOptions: {
      placeholderData: Array(3).fill(TX),
      select: (data) => data.slice(0, 3),
    },
  });

  return (
    <SocketProvider url={ getSocketUrl(chain?.app_config) }>
      <TxsContent
        items={ query.data }
        isPlaceholderData={ query.isPlaceholderData }
        isError={ query.isError }
        pagination={ PAGINATION_PARAMS }
        sort="default"
        socketType="txs_home"
        stickyHeader={ false }
      />
      <Separator orientation="horizontal" hideFrom="lg"/>
      <Link
        href={ route({ pathname: '/txs', query: { chain_id: chain?.id, tab: 'txs_local' } }) }
        w="full"
        justifyContent="center"
        textStyle="sm"
        mt={ 3 }
      >
        View all transactions
      </Link>
    </SocketProvider>
  );
};

export default React.memo(LatestTxsLocal);
