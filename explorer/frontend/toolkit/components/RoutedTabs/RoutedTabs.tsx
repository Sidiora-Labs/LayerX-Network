import { pickBy } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import type { Props as AdaptiveTabsProps } from '../AdaptiveTabs/AdaptiveTabs';
import AdaptiveTabs from '../AdaptiveTabs/AdaptiveTabs';
import { getTabValue } from '../AdaptiveTabs/utils';
import useActiveTabFromQuery from './useActiveTabFromQuery';

export interface RoutedTabsProps extends AdaptiveTabsProps {
  preservedParams?: Array<string>;
  defaultTabId?: string;
}

const RoutedTabs = (props: RoutedTabsProps) => {
  const { tabs, defaultTabId, onValueChange, preservedParams, ...rest } = props;

  const router = useRouter();
  const activeTab = useActiveTabFromQuery(tabs, defaultTabId);
  const tabsRef = React.useRef<HTMLDivElement>(null);

  const handleValueChange = React.useCallback(({ value }: { value: string }) => {
    const nextTab = tabs.find((tab) => getTabValue(tab) === value);

    if (!nextTab) {
      return;
    }

    const queryForPathname = pickBy(router.query, (_, key) => {
      if (preservedParams?.includes(String(key))) {
        return true;
      }

      return router.pathname.includes(`[${ key }]`);
    });
    router.push(
      { pathname: router.pathname, query: { ...queryForPathname, tab: value } },
      undefined,
      { shallow: true },
    );

    onValueChange?.({ value });
  }, [ tabs, router, onValueChange, preservedParams ]);

  React.useEffect(() => {
    if (router.query.scroll_to_tabs) {
      tabsRef?.current?.scrollIntoView(true);
      delete router.query.scroll_to_tabs;
      router.push(
        {
          pathname: router.pathname,
          query: router.query,
        },
        undefined,
        { shallow: true },
      );
    }
  // replicate componentDidMount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <AdaptiveTabs
      { ...rest }
      tabs={ tabs }
      onValueChange={ handleValueChange }
      defaultValue={ activeTab ? getTabValue(activeTab) : getTabValue(tabs[ 0 ]) }
    />
  );
};

export default React.memo(RoutedTabs);
