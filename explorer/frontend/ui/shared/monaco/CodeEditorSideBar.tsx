import { Box } from '@chakra-ui/react';
import { throttle } from 'es-toolkit';
import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import React from 'react';

import type { File, Monaco } from './types';

import type { TabsTriggerProps } from 'toolkit/chakra/tabs';
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'toolkit/chakra/tabs';
import { shift, cmd } from 'toolkit/utils/htmlEntities';

import CodeEditorFileExplorer from './CodeEditorFileExplorer';
import CodeEditorSearch from './CodeEditorSearch';
import useThemeColors from './utils/useThemeColors';

interface Props {
  monaco: Monaco | undefined;
  editor: monaco.editor.IStandaloneCodeEditor | undefined;
  data: Array<File>;
  onFileSelect: (index: number, lineNumber?: number) => void;
  selectedFile: string;
  mainFile?: string;
}

export const CONTAINER_WIDTH = 250;

const CodeEditorSideBar = ({ onFileSelect, data, monaco, editor, selectedFile, mainFile }: Props) => {

  const [ isStuck, setIsStuck ] = React.useState(false);
  const [ isDrawerOpen, setIsDrawerOpen ] = React.useState(false);
  const [ activeTab, setActiveTab ] = React.useState('explorer');
  const [ searchValue, setSearchValue ] = React.useState('');
  const [ actionBarRenderer, setActionBarRenderer ] = React.useState<() => React.JSX.Element>();

  const themeColors = useThemeColors();

  const tabProps: Partial<TabsTriggerProps> = {
    fontFamily: 'heading',
    textTransform: 'uppercase',
    fontSize: '11px',
    lineHeight: '35px',
    fontWeight: 500,
    color: themeColors['tab.inactiveForeground'],
    _selected: {
      color: themeColors['tab.activeForeground'],
    },
    px: 0,
    letterSpacing: 0.3,
  };

  const handleScrollThrottled = React.useRef(throttle((event: React.SyntheticEvent) => {
    setIsStuck((event.target as HTMLDivElement).scrollTop > 0);
  }, 100));

  const handleFileSelect = React.useCallback((index: number, lineNumber?: number) => {
    isDrawerOpen && setIsDrawerOpen(false);
    onFileSelect(index, lineNumber);
  }, [ isDrawerOpen, onFileSelect, setIsDrawerOpen ]);

  const handleSideBarButtonClick = React.useCallback(() => {
    setIsDrawerOpen((prev) => !prev);
  }, []);

  const handleTabChange = React.useCallback(({ value }: { value: string }) => {
    setActiveTab(value);
  }, []);

  React.useEffect(() => {
    if (editor && monaco) {
      editor.addAction({
        id: 'file-explorer',
        label: 'Show File Explorer',
        keybindings: [
          monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyE,
        ],
        contextMenuGroupId: 'navigation',
        contextMenuOrder: 1.5,
        run: function() {
          setActiveTab('explorer');
        },
      });

      editor.addAction({
        id: 'search-in-files',
        label: 'Show Search in Files',
        keybindings: [
          monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyF,
        ],
        contextMenuGroupId: 'navigation',
        contextMenuOrder: 1.6,
        run: function(editor) {
          setActiveTab('search');
          const selection = editor.getSelection();
          const selectedValue = selection ? editor.getModel()?.getValueInRange(selection) : '';
          setSearchValue(selectedValue || '');
        },
      });
    }
  }, [ editor, monaco ]);

  return (
    <>
      <Box
        w={ `${ CONTAINER_WIDTH }px` }
        flexShrink={ 0 }
        bgColor={ themeColors['sideBar.background'] }
        fontSize="13px"
        overflowY="scroll"
        onScroll={ handleScrollThrottled.current }
        position={{ base: 'absolute', lg: 'relative' }}
        right={{ base: isDrawerOpen ? '0' : `-${ CONTAINER_WIDTH }px`, lg: '0' }}
        top={{ base: 0, lg: undefined }}
        h="100%"
        pb="22px"
        boxShadow={{ base: isDrawerOpen ? 'md' : 'none', lg: 'none' }}
        zIndex={{ base: '2', lg: undefined }}
        transitionProperty="right"
        transitionDuration="normal"
        transitionTimingFunction="ease-in-out"
        borderTopRightRadius="md"
        borderBottomRightRadius="md"
      >
        <TabsRoot unmountOnExit={ false } variant="unstyled" size="free" value={ activeTab } onValueChange={ handleTabChange }>
          <TabsList
            columnGap={ 3 }
            position="sticky"
            top={ 0 }
            left={ 0 }
            bgColor={ themeColors['sideBar.background'] }
            zIndex="1"
            px={ 2 }
            h="35px"
            alignItems="center"
            boxShadow={ isStuck ? 'md' : 'none' }
            borderTopRightRadius="md"
          >
            <TabsTrigger value="explorer" { ...tabProps } title={ `File explorer (${ shift + cmd }E)` }>Explorer</TabsTrigger>
            <TabsTrigger value="search" { ...tabProps } title={ `Search in files (${ shift + cmd }F)` }>Search</TabsTrigger>
            { actionBarRenderer?.() }
          </TabsList>
          <TabsContent value="explorer" p={ 0 }>
            <CodeEditorFileExplorer
              data={ data }
              onFileSelect={ handleFileSelect }
              selectedFile={ selectedFile }
              mainFile={ mainFile }
              isActive={ activeTab === 'explorer' }
              setActionBarRenderer={ setActionBarRenderer }
            />
          </TabsContent>
          <TabsContent value="search" p={ 0 }>
            <CodeEditorSearch
              data={ data }
              onFileSelect={ handleFileSelect }
              monaco={ monaco }
              isInputStuck={ isStuck }
              isActive={ activeTab === 'search' }
              setActionBarRenderer={ setActionBarRenderer }
              defaultValue={ searchValue }
            />
          </TabsContent>
        </TabsRoot>
      </Box>
      <Box
        boxSize="24px"
        p="4px"
        position="absolute"
        display={{ base: 'block', lg: 'none' }}
        right={ isDrawerOpen ? `${ CONTAINER_WIDTH - 1 }px` : '0' }
        top="calc(50% - 12px)"
        backgroundColor={ themeColors['sideBar.background'] }
        borderTopLeftRadius="4px"
        borderBottomLeftRadius="4px"
        boxShadow="md"
        onClick={ handleSideBarButtonClick }
        zIndex="1"
        transitionProperty="right"
        transitionDuration="normal"
        transitionTimingFunction="ease-in-out"
        title={ isDrawerOpen ? 'Open sidebar' : 'Close sidebar' }
        aria-label={ isDrawerOpen ? 'Open sidebar' : 'Close sidebar' }
      >
        <Box
          className="codicon codicon-tree-item-expanded"
          transform={ isDrawerOpen ? 'rotate(-90deg)' : 'rotate(+90deg)' }
          boxSize="16px"
        />
      </Box>
    </>
  );
};

export default React.memo(CodeEditorSideBar);
