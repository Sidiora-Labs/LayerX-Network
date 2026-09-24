import { Flex } from '@chakra-ui/react';
import { useQuery } from '@tanstack/react-query';
import React from 'react';
import type { Chain, GetBlockReturnType, GetTransactionReturnType, TransactionReceipt } from 'viem';

import type { Transaction } from 'types/api/transaction';

import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import { publicClient } from 'lib/web3/client';
import formatTxData from 'lib/web3/rpc/formatTxData';
import { GET_BLOCK, GET_TRANSACTION, GET_TRANSACTION_RECEIPT, GET_TRANSACTION_CONFIRMATIONS } from 'stubs/RPC';
import { SECOND } from 'toolkit/utils/consts';
import ServiceDegradationWarning from 'ui/shared/alerts/ServiceDegradationWarning';
import TestnetWarning from 'ui/shared/alerts/TestnetWarning';
import isCustomAppError from 'ui/shared/AppError/isCustomAppError';
import DataFetchAlert from 'ui/shared/DataFetchAlert';

import TxInfo from './details/TxInfo';
import type { TxQuery } from './useTxQuery';

type RpcResponseType = [
  GetTransactionReturnType<Chain, 'latest'>,
  TransactionReceipt | null,
  bigint | null,
  GetBlockReturnType<Chain, false, 'latest'> | null,
];

interface Props {
  hash: string;
  txQuery: TxQuery;
}

const TxDetailsDegraded = ({ hash, txQuery }: Props) => {

  const [ originalError ] = React.useState(txQuery.error);

  const query = useQuery<RpcResponseType, unknown, Transaction | null>({
    queryKey: [ 'RPC', 'tx', { hash } ],
    queryFn: async() => {
      if (!publicClient) {
        throw new Error('No public RPC client');
      }

      const tx = await publicClient.getTransaction({ hash: hash as `0x${ string }` });

      if (!tx) {
        throw new Error('Not found');
      }

      const txReceipt = await publicClient.getTransactionReceipt({ hash: hash as `0x${ string }` }).catch(() => null);
      const block = await publicClient.getBlock({ blockHash: tx.blockHash }).catch(() => null);
      const latestBlock = await publicClient.getBlock().catch(() => null);
      const confirmations = latestBlock && block ? latestBlock.number - block.number + BigInt(1) : null;

      return [
        tx,
        txReceipt,
        confirmations,
        block,
      ];
    },
    select: (response) => {
      const [ tx, txReceipt, txConfirmations, block ] = response;

      return formatTxData(tx, txReceipt, txConfirmations, block);
    },
    placeholderData: [
      GET_TRANSACTION,
      GET_TRANSACTION_RECEIPT,
      GET_TRANSACTION_CONFIRMATIONS,
      GET_BLOCK,
    ],
    refetchOnMount: false,
    enabled: txQuery.isFetchedAfterMount,
    retry: 2,
    retryDelay: 5 * SECOND,
  });

  const hasData = Boolean(query.data);

  React.useEffect(() => {
    if (!query.isPlaceholderData && hasData) {
      txQuery.setRefetchEnabled(true);
    }
  }, [ hasData, query.isPlaceholderData, txQuery ]);

  React.useEffect(() => {
    return () => {
      txQuery.setRefetchEnabled(false);
    };
  }, [ txQuery ]);

  if (!query.data) {
    if (originalError && isCustomAppError(originalError)) {
      throwOnResourceLoadError({ resource: 'general:tx', error: originalError, isError: true });
    }

    return <DataFetchAlert/>;
  }

  return (
    <>
      <Flex rowGap={{ base: 1, lg: 2 }} mb={{ base: 3, lg: 6 }} flexDir="column">
        <TestnetWarning isLoading={ query.isPlaceholderData }/>
        { originalError?.status !== 404 && <ServiceDegradationWarning isLoading={ query.isPlaceholderData }/> }
      </Flex>
      <TxInfo data={ query.data } isLoading={ query.isPlaceholderData } noTxActions/>
    </>
  );
};

export default React.memo(TxDetailsDegraded);
