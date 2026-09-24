import { Flex, Text } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { IndexingStatus } from 'types/api/indexingStatus';

import config from 'configs/app';
import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { apos, nbsp, ndash } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

const IntTxsIndexingStatus = () => {

  const { data, isError, isPending } = useApiQuery('general:homepage_indexing_status', {
    queryOptions: {
      enabled: !config.UI.indexingAlert.intTxs.isHidden,
    },
  });

  const queryClient = useQueryClient();

  const handleInternalTxsIndexStatus: SocketMessage.InternalTxsIndexStatus['handler'] = React.useCallback((payload) => {
    queryClient.setQueryData(getResourceKey('general:homepage_indexing_status'), (prevData: IndexingStatus | undefined) => {

      const newData = prevData ? { ...prevData } : {} as IndexingStatus;
      newData.finished_indexing = payload.finished;
      newData.indexed_internal_transactions_ratio = payload.ratio;

      return newData;
    });
  }, [ queryClient ]);

  const internalTxsIndexingChannel = useSocketChannel({
    topic: 'blocks:indexing_internal_transactions',
    isDisabled: !data || data.finished_indexing,
  });

  useSocketMessage({
    channel: internalTxsIndexingChannel,
    event: 'index_status',
    handler: handleInternalTxsIndexStatus,
  });

  if (isError || isPending) {
    return null;
  }

  if (data.finished_indexing !== false) {
    return null;
  }

  const hint = (
    <Text textStyle="xs">
      { data.indexed_internal_transactions_ratio &&
        `${ Math.floor(Number(data.indexed_internal_transactions_ratio) * 100) }% Blocks With Internal Transactions Indexed${ nbsp }${ ndash } ` }
      We{ apos }re indexing this chain right now. Some of the counts may be inaccurate.
    </Text>
  );

  const trigger = (
    <Flex
      px={ 1 }
      bg={{ base: 'blackAlpha.50', _dark: 'whiteAlpha.100' }}
      borderRadius="sm"
      alignItems="center"
      justifyContent="center"
      color="green.400"
      _hover={{ color: 'hover' }}
    >
      <IconSvg name="info" boxSize={ 5 }/>
      { data.indexed_internal_transactions_ratio && (
        <Text fontWeight={ 600 } textStyle="xs" color="inherit">
          { Math.floor(Number(data.indexed_internal_transactions_ratio) * 100) + '%' }
        </Text>
      ) }
    </Flex>
  );

  return (
    <Tooltip content={ hint } interactive positioning={{ placement: 'bottom-start' }} lazyMount>
      { trigger }
    </Tooltip>
  );
};

export default IntTxsIndexingStatus;
