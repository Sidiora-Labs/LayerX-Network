import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import PageTitle from 'ui/shared/Page/PageTitle';
import TxsStats from 'ui/txs/TxsStats';
import TxsTabs, { getTabId } from 'ui/txs/TxsTabs';

import TransactionsCrossChain from './TransactionsCrossChain';

const LOCAL_TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: { base: 9, lg: 3 },
};
const LOCAL_TAB_ID = 'txs_local';
const LOCAL_SUB_TABS = [ getTabId('validated', LOCAL_TAB_ID), getTabId('pending', LOCAL_TAB_ID), getTabId('blob_txs', LOCAL_TAB_ID) ];

const Transactions = () => {
  const tabs: Array<TabItemRegular> = React.useMemo(() => {
    return [
      {
        id: LOCAL_TAB_ID,
        title: 'Txns',
        component: (
          <>
            <TxsStats mb={ 0 }/>
            <TxsTabs
              parentTab={ LOCAL_TAB_ID }
              variant="secondary"
              size="sm"
              tabsHeight={ ACTION_BAR_HEIGHT_DESKTOP }
              listProps={ LOCAL_TAB_LIST_PROPS }
            />
          </>
        ),
        subTabs: LOCAL_SUB_TABS,
      },
      {
        id: 'txs_cross_chain',
        title: 'Cross-chain txns',
        component: <TransactionsCrossChain/>,
      },
    ];
  }, [ ]);

  return (
    <>
      <PageTitle withTextAd title="Transactions"/>
      <RoutedTabs tabs={ tabs }/>
    </>
  );
};

export default React.memo(Transactions);
