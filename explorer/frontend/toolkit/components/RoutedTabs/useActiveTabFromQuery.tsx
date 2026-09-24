import { useRouter } from 'next/router';

import type { TabItem } from '../AdaptiveTabs/types';

import { castToString } from '../../utils/guards';

export default function useActiveTabFromQuery(tabs: Array<TabItem>, defaultTabId?: string) {
  const router = useRouter();
  const tabFromQuery = castToString(router.query.tab);

  if (!tabFromQuery) {
    if (defaultTabId) {
      return tabs.find((tab) => tab.id === defaultTabId);
    }

    return;
  }

  return tabs.find((tab) => {
    if (Array.isArray(tab.id)) {
      return tab.id.includes(tabFromQuery);
    }

    return tab.id === tabFromQuery || ('subTabs' in tab && tab.subTabs?.some((id) => id === tabFromQuery));
  });
}
