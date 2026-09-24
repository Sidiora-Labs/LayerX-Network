import type { TabItem, TabItemMenu } from './types';

import { middot } from '../../utils/htmlEntities';

export const menuButton: TabItemMenu = {
  id: 'menu',
  title: `${ middot }${ middot }${ middot }`,
  component: null,
};

export const getTabValue = (tab: TabItem | undefined): string => {
  if (!tab) {
    return 'undefined';
  }

  if (Array.isArray(tab.id)) {
    return tab.id[0];
  }

  return tab.id;
};
