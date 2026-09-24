import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import { MultichainProvider } from 'lib/contexts/multichain';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import getQueryParamString from 'lib/router/getQueryParamString';
import { EmptyState } from 'toolkit/chakra/empty-state';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import PageTitle from 'ui/shared/Page/PageTitle';

import MultichainTxsLocal, { MULTICHAIN_TXS_LOCAL_TAB_IDS } from './MultichainTxsLocal';

const TABS_RIGHT_SLOT_PROPS = {
  ml: { base: 'auto', lg: 6 },
};
const QUERY_PRESERVED_PARAMS = [ 'tab' ];

const MultichainTxs = () => {
  const router = useRouter();
  const chainSelect = useRoutedChainSelect({ persistedParams: QUERY_PRESERVED_PARAMS });

  const tab = getQueryParamString(router.query.tab);
  const isLocalTxs = tab === 'txs_local' || MULTICHAIN_TXS_LOCAL_TAB_IDS.includes(tab) || !tab;

  const tabs: Array<TabItemRegular> = React.useMemo(() => {
    return [
      {
        id: 'txs_cross_chain',
        title: 'Cross-chain',
        component: <EmptyState type="coming_soon"/>,
      },
      {
        id: 'txs_local',
        title: 'Local',
        component: (
          <MultichainProvider chainId={ chainSelect.value?.[0] }>
            <MultichainTxsLocal/>
          </MultichainProvider>
        ),
        subTabs: MULTICHAIN_TXS_LOCAL_TAB_IDS,
      },
    ];
  }, [ chainSelect.value ]);

  const rightSlot = isLocalTxs && (
    <ChainSelect
      value={ chainSelect.value }
      onValueChange={ chainSelect.onValueChange }
    />
  );

  return (
    <>
      <PageTitle
        withTextAd
        title="Transactions"
      />
      <RoutedTabs
        tabs={ tabs }
        defaultTabId="txs_local"
        rightSlot={ rightSlot }
        rightSlotProps={ rightSlot ? TABS_RIGHT_SLOT_PROPS : undefined }
      />
    </>
  );
};

export default React.memo(MultichainTxs);
