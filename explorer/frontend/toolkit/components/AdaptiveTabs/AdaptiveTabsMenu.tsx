import { Icon } from '@chakra-ui/react';
import React from 'react';

import type { TabItem } from './types';

import DotsIcon from 'icons/dots.svg';

import { IconButton } from '../../chakra/icon-button';
import type { IconButtonProps } from '../../chakra/icon-button';
import { PopoverBody, PopoverCloseTriggerWrapper, PopoverContent, PopoverRoot, PopoverTrigger } from '../../chakra/popover';
import { TabsCounter, TabsTrigger } from '../../chakra/tabs';
import { getTabValue } from './utils';

interface Props extends IconButtonProps {
  tabs: Array<TabItem>;
  tabsCut: number;
  isActive: boolean;
}

const AdaptiveTabsMenu = ({ tabs, tabsCut, isActive, ...props }: Props, ref: React.Ref<HTMLButtonElement>) => {

  return (
    <PopoverRoot positioning={{ placement: 'bottom-end' }}>
      <PopoverTrigger>
        <IconButton
          // we use "div" so the :last-of-type pseudo-class targets the last tab and not the menu trigger
          as="div"
          variant="plain"
          color="tabs.solid.fg"
          _hover={{
            color: 'hover',
          }}
          _expanded={{
            color: 'selected.control.text',
            bg: 'selected.control.bg',
          }}
          ref={ ref }
          expanded={ isActive }
          px="18px"
          aria-label="Open tabs menu"
          { ...props }
        >
          <Icon boxSize={ 5 }><DotsIcon/></Icon>
        </IconButton>
      </PopoverTrigger>
      <PopoverContent>
        <PopoverBody display="flex" flexDir="column" rowGap={ 2 } px={ 0 }>
          { tabs.slice(tabsCut).map((tab) => {
            const value = getTabValue(tab);

            return (
              <PopoverCloseTriggerWrapper key={ value }>
                <TabsTrigger
                  className="group"
                  value={ value }
                  w="100%"
                  py="5px"
                  borderRadius="none"
                  fontWeight="normal"
                  color="tabs.solid.fg"
                  _hover={{
                    bg: 'selected.control.bg',
                  }}
                >
                  { typeof tab.title === 'function' ? tab.title() : tab.title }
                  <TabsCounter count={ tab.count }/>
                </TabsTrigger>
              </PopoverCloseTriggerWrapper>
            );
          }) }
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(React.forwardRef(AdaptiveTabsMenu));
