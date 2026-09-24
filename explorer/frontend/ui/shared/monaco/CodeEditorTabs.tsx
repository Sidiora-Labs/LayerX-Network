import { Flex } from '@chakra-ui/react';
import React from 'react';

import CodeEditorTab from './CodeEditorTab';
import useThemeColors from './utils/useThemeColors';

interface Props {
  tabs: Array<string>;
  activeTab: string;
  mainFile?: string;
  onTabSelect: (tab: string) => void;
  onTabClose: (tab: string) => void;
}

const CodeEditorTabs = ({ tabs, activeTab, mainFile, onTabSelect, onTabClose }: Props) => {
  const themeColors = useThemeColors();

  const tabsPathChunks = React.useMemo(() => {
    return tabs.map((tab) => tab.split('/'));
  }, [ tabs ]);

  return (
    <Flex
      borderTopLeftRadius="md"
      overflow="hidden"
      bgColor={ themeColors['sideBar.background'] }
      flexWrap="wrap"
    >
      { tabs.map((tab) => (
        <CodeEditorTab
          key={ tab }
          path={ tab }
          isActive={ activeTab === tab }
          isMainFile={ mainFile === tab }
          onClick={ onTabSelect }
          onClose={ onTabClose }
          isCloseDisabled={ tabs.length === 1 }
          tabsPathChunks={ tabsPathChunks }
        />
      )) }
    </Flex>
  );
};

export default React.memo(CodeEditorTabs);
