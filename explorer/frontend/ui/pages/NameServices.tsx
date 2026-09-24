import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import config from 'configs/app';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import Clusters from 'ui/nameServices/directories/Clusters';
import NameDomains from 'ui/nameServices/domains/NameDomains';
import PageTitle from 'ui/shared/Page/PageTitle';

const feature = config.features.nameServices;

const NameServices = () => {
  const tabs: Array<TabItemRegular> = [
    feature.isEnabled && feature.ens.isEnabled && { id: 'domains', title: 'Domains', component: <NameDomains/> },
    feature.isEnabled && feature.clusters.isEnabled && { id: 'directories', title: 'Directories', component: <Clusters/> },
  ].filter(Boolean);

  return (
    <>
      <PageTitle
        title={ config.meta.seo.enhancedDataEnabled ? `${ config.chain.name } name services` : 'Name services lookup' }
        withTextAd
      />
      <RoutedTabs tabs={ tabs }/>
    </>
  );
};

export default NameServices;
