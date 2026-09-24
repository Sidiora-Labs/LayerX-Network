import React from 'react';

import type { TabsProps } from '../../chakra/tabs';
import { TabsContent, TabsRoot } from '../../chakra/tabs';
import { useViewportSize } from '../../hooks/useViewportSize';
import AdaptiveTabsList, { type BaseProps as AdaptiveTabsListProps } from './AdaptiveTabsList';
import { getTabValue } from './utils';

export interface Props extends TabsProps, AdaptiveTabsListProps { }

const AdaptiveTabs = (props: Props) => {
  const {
    tabs,
    onValueChange,
    defaultValue,
    isLoading,
    listProps,
    rightSlot,
    rightSlotProps,
    leftSlot,
    leftSlotProps,
    stickyEnabled,
    size,
    variant,
    ...rest
  } = props;

  const [ activeTab, setActiveTab ] = React.useState<string>(defaultValue || getTabValue(tabs[0]));

  const handleTabChange = React.useCallback(({ value }: { value: string }) => {
    if (isLoading) {
      return;
    }
    onValueChange ? onValueChange({ value }) : setActiveTab(value);
  }, [ isLoading, onValueChange ]);

  const viewportSize = useViewportSize();

  React.useEffect(() => {
    if (defaultValue) {
      setActiveTab(defaultValue);
    }
  }, [ defaultValue ]);

  return (
    <TabsRoot
      position="relative"
      value={ activeTab }
      onValueChange={ handleTabChange }
      size={ size }
      variant={ variant }
      { ...rest }
    >
      <AdaptiveTabsList
      // the easiest and most readable way to achieve correct tab's cut recalculation when
        //    - screen is resized or
        //    - tabs list is changed when API data is loaded
        // is to do full re-render of the tabs list
        // so we use screenWidth + tabIds as a key for the TabsList component
        key={ isLoading + '_' + viewportSize.width + '_' + tabs.map((tab) => tab.id).join(':') }
        tabs={ tabs }
        listProps={ listProps }
        leftSlot={ leftSlot }
        leftSlotProps={ leftSlotProps }
        rightSlot={ rightSlot }
        rightSlotProps={ rightSlotProps }
        stickyEnabled={ stickyEnabled }
        activeTab={ activeTab }
        isLoading={ isLoading }
        variant={ variant }
      />
      { tabs.map((tab) => {
        const value = getTabValue(tab);
        return (
          <TabsContent padding={ 0 } key={ value } value={ value }>
            { tab.component }
          </TabsContent>
        );
      }) }
    </TabsRoot>
  );
};

export default React.memo(AdaptiveTabs);
