import React from 'react';

import { USER_OPS_ITEM } from 'stubs/userOps';
import { generateListStub } from 'stubs/utils';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TxPendingAlert from 'ui/tx/TxPendingAlert';
import TxSocketAlert from 'ui/tx/TxSocketAlert';
import UserOpsContent from 'ui/userOps/UserOpsContent';

import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
}

const TxUserOps = ({ txQuery }: Props) => {
  const userOpsQuery = useQueryWithPages({
    resourceName: 'general:user_ops',
    options: {
      enabled: !txQuery.isPlaceholderData && Boolean(txQuery.data?.status && txQuery.data?.hash),
      // most often there is only one user op in one tx
      placeholderData: generateListStub<'general:user_ops'>(USER_OPS_ITEM, 1, { next_page_params: null }),
    },
    filters: { transaction_hash: txQuery.data?.hash },
  });

  if (!txQuery.isPending && !txQuery.isPlaceholderData && !txQuery.isError && !txQuery.data.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  if (txQuery.isError) {
    return <DataFetchAlert/>;
  }

  return <UserOpsContent query={ userOpsQuery } showTx={ false }/>;
};

export default TxUserOps;
