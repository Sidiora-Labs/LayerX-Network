import { GridItem } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import { ZKSYNC_L2_TX_BATCH_STATUSES, type ZkSyncBatch } from 'types/api/zkSyncL2';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import { formatZkSyncL2TxnBatchStatus, layerLabels } from 'lib/rollups/utils';
import { currencyUnits } from 'lib/units';
import { CollapsibleDetails } from 'toolkit/chakra/collapsible';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import isCustomAppError from 'ui/shared/AppError/isCustomAppError';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import PrevNext from 'ui/shared/PrevNext';
import GasPriceValue from 'ui/shared/value/GasPriceValue';
import VerificationSteps from 'ui/shared/verificationSteps/VerificationSteps';

import ZkSyncL2TxnBatchHashesInfo from './ZkSyncL2TxnBatchHashesInfo';

const rollupFeature = config.features.rollup;

interface Props {
  query: UseQueryResult<ZkSyncBatch, ResourceError>;
}

const ZkSyncL2TxnBatchDetails = ({ query }: Props) => {
  const router = useRouter();

  const { data, isPlaceholderData, isError, error } = query;

  const handlePrevNextClick = React.useCallback((direction: 'prev' | 'next') => {
    if (!data) {
      return;
    }

    const increment = direction === 'next' ? +1 : -1;
    const nextId = String(data.number + increment);

    router.push({ pathname: '/batches/[number]', query: { number: nextId } }, undefined);
  }, [ data, router ]);

  if (isError) {
    if (isCustomAppError(error)) {
      throwOnResourceLoadError({ isError, error });
    }

    return <DataFetchAlert/>;
  }

  if (!data) {
    return null;
  }

  const txNum = data.l2_transactions_count + data.l1_transactions_count;
  const parentChainCurrency = rollupFeature.isEnabled ? rollupFeature.parentChain.currency?.symbol : undefined;

  return (
    <DetailedInfo.Container
      templateColumns={{ base: 'minmax(0, 1fr)', lg: 'minmax(min-content, 200px) minmax(0, 1fr)' }}
    >
      <DetailedInfo.ItemLabel
        hint={ `Batch number indicates the length of batches produced by grouping ${ layerLabels.current } blocks to be proven on ${ layerLabels.parent }.` }
        isLoading={ isPlaceholderData }
      >
        Txn batch number
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.number }
        </Skeleton>
        <PrevNext
          ml={ 6 }
          onClick={ handlePrevNextClick }
          prevLabel="View previous txn batch"
          nextLabel="View next txn batch"
          isPrevDisabled={ data.number === 0 }
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Status is the short interpretation of the batch lifecycle"
        isLoading={ isPlaceholderData }
      >
        Status
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <VerificationSteps
          steps={ ZKSYNC_L2_TX_BATCH_STATUSES.slice(1).map(formatZkSyncL2TxnBatchStatus) }
          currentStep={ formatZkSyncL2TxnBatchStatus(data.status) }
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Date and time at which batch is produced"
        isLoading={ isPlaceholderData }
      >
        Timestamp
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        { data.timestamp ? <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ isPlaceholderData }/> : 'Undefined' }
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Number of transactions inside the batch."
        isLoading={ isPlaceholderData }
      >
        Transactions
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          <Link href={ route({ pathname: '/batches/[number]', query: { number: data.number.toString(), tab: 'txs' } }) }>
            { txNum } transaction{ txNum === 1 ? '' : 's' }
          </Link>
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemDivider/>

      <ZkSyncL2TxnBatchHashesInfo isLoading={ isPlaceholderData } data={ data }/>

      <CollapsibleDetails loading={ isPlaceholderData } mt={ 6 } gridColumn={{ base: undefined, lg: '1 / 3' }}>
        <GridItem colSpan={{ base: undefined, lg: 2 }} mt={{ base: 1, lg: 4 }}/>

        <DetailedInfo.ItemLabel
          hint={ `${ layerLabels.parent } batch root is a hash that summarizes batch data and submitted to ${ layerLabels.parent }` }
        >
          Root hash
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue
          flexWrap="nowrap"
          alignSelf="flex-start"
        >
          <TruncatedText text={ data.root_hash }/>
          <CopyToClipboard text={ data.root_hash }/>
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemLabel
          hint={ `Gas price for the batch settlement transaction on ${ layerLabels.parent }` }
        >
          { layerLabels.parent } gas price
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue multiRow>
          <GasPriceValue
            amount={ data.l1_gas_price }
            loading={ isPlaceholderData }
            asset={ parentChainCurrency || currencyUnits.ether }
          />
        </DetailedInfo.ItemValue>

        <DetailedInfo.ItemLabel
          hint='The gas price below which the "baseFee" of the batch should not fall'
        >
          { layerLabels.current } fair gas price
        </DetailedInfo.ItemLabel>
        <DetailedInfo.ItemValue multiRow>
          <GasPriceValue
            amount={ data.l2_fair_gas_price }
            loading={ isPlaceholderData }
          />
        </DetailedInfo.ItemValue>
      </CollapsibleDetails>
    </DetailedInfo.Container>
  );
};

export default ZkSyncL2TxnBatchDetails;
