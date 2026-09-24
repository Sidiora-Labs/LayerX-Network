import React from 'react';

interface Props {
  activeTabIndex: number;
  tabsRefs: Array<React.RefObject<HTMLButtonElement>>;
  listRef: React.RefObject<HTMLDivElement | null>;
  isMobile?: boolean;
  isLoading?: boolean;
}

export default function useScrollToActiveTab({ activeTabIndex, tabsRefs, listRef, isMobile, isLoading }: Props) {
  React.useEffect(() => {
    if (isLoading) {
      return;
    }

    if (activeTabIndex < tabsRefs.length && isMobile) {
      window.setTimeout(() => {
        const activeTabRef = tabsRefs[activeTabIndex];

        if (activeTabRef.current && listRef.current) {
          const containerWidth = listRef.current.getBoundingClientRect().width;
          const activeTabWidth = activeTabRef.current.getBoundingClientRect().width;
          const left = tabsRefs.slice(0, activeTabIndex)
            .map((tab) => tab.current?.getBoundingClientRect())
            .filter(Boolean)
            .map((rect) => rect.width)
            .reduce((result, item) => result + item, 0);

          const isWithinFirstPage = containerWidth > left + activeTabWidth;

          if (isWithinFirstPage) {
            listRef.current.scrollTo({ left: 0 });
            return;
          }

          listRef.current.scrollTo({
            left,
            behavior: 'smooth',
          });
        }

      // have to wait until DOM is updated and all styles to tabs is applied
      }, 300);
    }
  // run only when tab index or device type is changed
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ activeTabIndex, isMobile, isLoading ]);
}
