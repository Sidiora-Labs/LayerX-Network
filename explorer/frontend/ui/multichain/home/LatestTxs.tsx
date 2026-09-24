import { useRouter } from 'next/router';
import React from 'react';

import { MultichainProvider } from 'lib/contexts/multichain';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import getQueryParamString from 'lib/router/getQueryParamString';
import { EmptyState } from 'toolkit/chakra/empty-state';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import ChainSelect from 'ui/multichain/components/ChainSelect';

import LatestTxsLocal from './LatestTxsLocal';

const LatestTxs = () => {
  const router = useRouter();
  const tab = getQueryParamString(router.query.tab);
  const chainSelect = useRoutedChainSelect();

  const isLocalTab = tab === 'txs_local' || !tab;

  const tabs = [
    {
      id: 'cross_chain_txs',
      title: 'Cross-chain txns',
      component: <EmptyState type="coming_soon" my={ 6 }/>,
    },
    {
      id: 'txs_local',
      title: 'Local txns',
      component: chainSelect.value ? (
        <MultichainProvider chainId={ chainSelect.value[0] }>
          <LatestTxsLocal/>
        </MultichainProvider>
      ) : null,
    },
  ];

  const rightSlot = isLocalTab ? (
    <ChainSelect
      loading={ false }
      value={ chainSelect.value }
      onValueChange={ chainSelect.onValueChange }
      w="max-content"
      ml={{ base: 'auto', lg: 0 }}
    />
  ) : null;

  return (
    <RoutedTabs
      tabs={ tabs }
      defaultTabId="txs_local"
      listProps={{ mb: 3 }}
      leftSlotProps={{ mr: 6 }}
      rightSlot={ rightSlot }
      rightSlotProps={{ ml: 6, widthAllocation: 'available' }}
      my={ 8 }
    />
  );
};

export default React.memo(LatestTxs);
