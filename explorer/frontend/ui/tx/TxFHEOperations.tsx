import { Box } from '@chakra-ui/react';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import { FHE_OPERATIONS_RESPONSE } from 'stubs/fheOperations';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import DataListDisplay from 'ui/shared/DataListDisplay';
import TxFHEOperationsList from 'ui/tx/fheOperations/TxFHEOperationsList';
import TxFHEOperationsStats from 'ui/tx/fheOperations/TxFHEOperationsStats';
import TxFHEOperationsTable from 'ui/tx/fheOperations/TxFHEOperationsTable';
import TxPendingAlert from 'ui/tx/TxPendingAlert';
import TxSocketAlert from 'ui/tx/TxSocketAlert';

import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
}

const TxFHEOperations = ({ txQuery }: Props) => {
  const hash = txQuery.data?.hash || '';
  const isEnabled = Boolean(hash) && Boolean(txQuery.data?.status) && !txQuery.isPlaceholderData;

  const { data, isError, isPlaceholderData } = useApiQuery('general:tx_fhe_operations', {
    pathParams: { hash },
    queryOptions: {
      enabled: isEnabled,
      retry: false,
      placeholderData: FHE_OPERATIONS_RESPONSE,
    },
  });

  if (!txQuery.isPending && !txQuery.isPlaceholderData && !txQuery.isError && txQuery.data && !txQuery.data.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  if (txQuery.isError || isError) {
    return <DataFetchAlert/>;
  }

  const content = data ? (
    <>
      <TxFHEOperationsStats
        totalHcu={ data.total_hcu }
        maxDepthHcu={ data.max_depth_hcu }
        operationCount={ data.operation_count }
        isLoading={ isPlaceholderData }
      />
      <Box>
        <TxFHEOperationsTable data={ data.items } isLoading={ isPlaceholderData }/>
        <TxFHEOperationsList data={ data.items } isLoading={ isPlaceholderData }/>
      </Box>
    </>
  ) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items?.length ?? 0 }
      emptyText="There are no FHE operations for this transaction."
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(TxFHEOperations);
