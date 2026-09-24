import { Text } from '@chakra-ui/react';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import config from 'configs/app';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import EthRpcApi from 'ui/apiDocs/EthRpcApi';
import GraphQL from 'ui/apiDocs/GraphQL';
import RestApi from 'ui/apiDocs/RestApi';
import RpcApi from 'ui/apiDocs/RpcApi';
import AlertWithExternalHtml from 'ui/shared/alerts/AlertWithExternalHtml';
import PageTitle from 'ui/shared/Page/PageTitle';

const feature = config.features.apiDocs;

const ApiDocs = () => {

  const tabs: Array<TabItemRegular> = [
    { id: 'rest_api', title: 'REST API', component: <RestApi/> },
    { id: 'eth_rpc_api', title: 'ETH RPC API', component: <EthRpcApi/> },
    { id: 'rpc_api', title: 'RPC API endpoints', component: <RpcApi/> },
    { id: 'graphql_api', title: 'GraphQL API', component: <GraphQL/> },
  ].filter(({ id }) => feature.isEnabled && feature.tabs.includes(id));

  return (
    <>
      <PageTitle
        title={ config.meta.seo.enhancedDataEnabled ? `${ config.chain.name } API documentation` : 'API documentation' }
      />
      { feature.isEnabled && feature.alertMessage ? <AlertWithExternalHtml html={ feature.alertMessage } status="info" showIcon mb={ 6 }/> : null }
      { tabs.length > 0 ? <RoutedTabs tabs={ tabs }/> : <Text>No API documentation available</Text> }
    </>
  );
};

export default React.memo(ApiDocs);
