import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import PageTitle from 'ui/shared/Page/PageTitle';
import TxsStats from 'ui/txs/TxsStats';
import TxsTabs from 'ui/txs/TxsTabs';

const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 6,
  marginTop: -5,
};
const TABS_HEIGHT = 88;

const Transactions = () => {
  const isMobile = useIsMobile();

  return (
    <>
      <PageTitle
        title={ config.meta.seo.enhancedDataEnabled ? `${ config.chain.name } transactions` : 'Transactions' }
        withTextAd
      />
      <TxsStats/>
      <TxsTabs
        listProps={ isMobile ? undefined : TAB_LIST_PROPS }
        tabsHeight={ TABS_HEIGHT }
      />
    </>
  );
};

export default Transactions;
