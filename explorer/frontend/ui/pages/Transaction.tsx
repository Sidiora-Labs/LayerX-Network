import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';
import type { EntityTag as TEntityTag } from 'ui/shared/EntityTags/types';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import useEtherscanRedirects from 'lib/router/useEtherscanRedirects';
import { publicClient } from 'lib/web3/client';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import TextAd from 'ui/shared/ad/TextAd';
import isCustomAppError from 'ui/shared/AppError/isCustomAppError';
import EntityTags from 'ui/shared/EntityTags/EntityTags';
import PageTitle from 'ui/shared/Page/PageTitle';
import TxAssetFlows from 'ui/tx/TxAssetFlows';
import TxAuthorizations from 'ui/tx/TxAuthorizations';
import TxBlobs from 'ui/tx/TxBlobs';
import TxDetails from 'ui/tx/TxDetails';
import TxDetailsDegraded from 'ui/tx/TxDetailsDegraded';
import TxDetailsWrapped from 'ui/tx/TxDetailsWrapped';
import TxFHEOperations from 'ui/tx/TxFHEOperations';
import TxInternals from 'ui/tx/TxInternals';
import TxLogs from 'ui/tx/TxLogs';
import TxRawTrace from 'ui/tx/TxRawTrace';
import TxState from 'ui/tx/TxState';
import TxSubHeading from 'ui/tx/TxSubHeading';
import TxTokenTransfer from 'ui/tx/TxTokenTransfer';
import TxUserOps from 'ui/tx/TxUserOps';
import useTxQuery from 'ui/tx/useTxQuery';

const txInterpretation = config.features.txInterpretation;
const rollupFeature = config.features.rollup;
const tacFeature = config.features.tac;

const TransactionPageContent = () => {
  const router = useRouter();

  const hash = getQueryParamString(router.query.hash);

  useEtherscanRedirects();

  const txQuery = useTxQuery();

  const tacOperationQuery = useApiQuery('tac:operation_by_tx_hash', {
    pathParams: { tx_hash: hash },
    queryOptions: {
      enabled: tacFeature.isEnabled,
    },
  });

  const { data, isPlaceholderData, isError, error, errorUpdateCount } = txQuery;

  const showDegradedView = publicClient && ((isError && error.status !== 422) || isPlaceholderData) && errorUpdateCount > 0;

  const tabs: Array<TabItemRegular> = (() => {
    const detailsComponent = showDegradedView ?
      <TxDetailsDegraded hash={ hash } txQuery={ txQuery }/> :
      <TxDetails txQuery={ txQuery } tacOperationQuery={ tacFeature.isEnabled ? tacOperationQuery : undefined }/>;

    return [
      {
        id: 'index',
        title: config.features.suave.isEnabled && data?.wrapped ? 'Confidential compute tx details' : 'Details',
        component: detailsComponent,
      },
      txInterpretation.isEnabled && txInterpretation.provider === 'noves' ?
        { id: 'asset_flows', title: 'Asset Flows', component: <TxAssetFlows hash={ hash }/> } :
        undefined,
      config.features.suave.isEnabled && data?.wrapped ?
        { id: 'wrapped', title: 'Regular tx details', component: <TxDetailsWrapped data={ data.wrapped }/> } :
        undefined,
      {
        id: 'token_transfers',
        title: 'Token transfers',
        component: <TxTokenTransfer txQuery={ txQuery }/>,
        subTabs: [ 'token_transfers', 'token_transfers_cross_chain' ],
      },
      config.features.userOps.isEnabled ?
        { id: 'user_ops', title: 'User operations', component: <TxUserOps txQuery={ txQuery }/> } :
        undefined,
      { id: 'internal', title: 'Internal txns', component: <TxInternals txQuery={ txQuery }/> },
      config.features.dataAvailability.isEnabled && txQuery.data?.blob_versioned_hashes?.length ?
        { id: 'blobs', title: 'Blobs', component: <TxBlobs txQuery={ txQuery }/> } :
        undefined,
      { id: 'logs', title: 'Logs', component: <TxLogs txQuery={ txQuery }/> },
      { id: 'state', title: 'State', component: <TxState txQuery={ txQuery }/> },
      { id: 'raw_trace', title: 'Raw trace', component: <TxRawTrace txQuery={ txQuery }/> },
      txQuery.data?.fhe_operations_count && txQuery.data.fhe_operations_count > 0 ?
        { id: 'fhe_operations', title: 'FHE operations', component: <TxFHEOperations txQuery={ txQuery }/> } :
        undefined,
      txQuery.data?.authorization_list?.length ?
        { id: 'authorizations', title: 'Authorizations', component: <TxAuthorizations txQuery={ txQuery }/> } :
        undefined,
    ].filter(Boolean);
  })();

  const txTags: Array<TEntityTag> = data?.transaction_tag ?
    [ { slug: data.transaction_tag, name: data.transaction_tag, tagType: 'private_tag' as const, ordinal: 1 } ] : [];

  if (rollupFeature.isEnabled && rollupFeature.interopEnabled && data?.op_interop_messages && data.op_interop_messages.length > 0) {
    if (data.op_interop_messages.some(message => message.init_chain !== undefined)) {
      txTags.push({ slug: 'relay_tx', name: 'Relay tx', tagType: 'custom' as const, ordinal: 0 });
    }
    if (data.op_interop_messages.some(message => message.relay_chain !== undefined)) {
      txTags.push({ slug: 'init_tx', name: 'Source tx', tagType: 'custom' as const, ordinal: 0 });
    }
  }

  const protocolTags = data?.to?.metadata?.tags?.filter(tag => tag.tagType === 'protocol');
  if (protocolTags && protocolTags.length > 0) {
    txTags.push(...protocolTags);
  }

  const tags = (
    <EntityTags
      isLoading={ !txQuery.isFetchedAfterMount || (tacFeature.isEnabled && tacOperationQuery.isPlaceholderData) }
      tags={ txTags }
    />
  );

  const titleSecondRow = <TxSubHeading hash={ hash } hasTag={ Boolean(data?.transaction_tag) } txQuery={ txQuery }/>;

  if (isError && !showDegradedView) {
    if (isCustomAppError(error)) {
      throwOnResourceLoadError({ resource: 'general:tx', error, isError: true });
    }
  }

  return (
    <AddressHighlightProvider>
      <TextAd mb={ 6 }/>
      <PageTitle
        title="Transaction details"
        contentAfter={ tags }
        secondRow={ titleSecondRow }
      />
      <RoutedTabs tabs={ tabs } isLoading={ !txQuery.isFetchedAfterMount }/>
    </AddressHighlightProvider>
  );
};

export default TransactionPageContent;
