import type { SystemStyleObject } from '@chakra-ui/react';
import { Box, Flex, useToken, Center } from '@chakra-ui/react';
import type { EditorProps } from '@monaco-editor/react';
import MonacoEditor from '@monaco-editor/react';
import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import React from 'react';

import type { File, Monaco } from './types';
import type { SmartContractExternalLibrary } from 'types/api/contract';

import useIsMobile from 'lib/hooks/useIsMobile';
import isMetaKey from 'lib/isMetaKey';
import { useColorMode } from 'toolkit/chakra/color-mode';
import { useClientRect } from 'toolkit/hooks/useClientRect';
import ErrorBoundary from 'ui/shared/ErrorBoundary';

import CodeEditorBreadcrumbs from './CodeEditorBreadcrumbs';
import CodeEditorLoading from './CodeEditorLoading';
import CodeEditorSideBar, { CONTAINER_WIDTH as SIDE_BAR_WIDTH } from './CodeEditorSideBar';
import CodeEditorTabs from './CodeEditorTabs';
import addExternalLibraryWarningDecoration from './utils/addExternalLibraryWarningDecoration';
import addFileImportDecorations from './utils/addFileImportDecorations';
import addMainContractCodeDecoration from './utils/addMainContractCodeDecoration';
import { defGeas, configGeas } from './utils/defGeas';
import { defScilla, configScilla } from './utils/defScilla';
import getFullPathOfImportedFile from './utils/getFullPathOfImportedFile';
import * as themes from './utils/themes';
import useThemeColors from './utils/useThemeColors';
const EDITOR_OPTIONS: EditorProps['options'] = {
  readOnly: true,
  minimap: { enabled: false },
  scrollbar: {
    alwaysConsumeMouseWheel: true,
  },
  dragAndDrop: false,
};

const TABS_HEIGHT = 35;
const BREADCRUMBS_HEIGHT = 22;
const EDITOR_HEIGHT = 500;

interface Props {
  data: Array<File>;
  remappings?: Array<string>;
  libraries?: Array<SmartContractExternalLibrary>;
  language?: string;
  mainFile?: string;
  contractName?: string;
}

const CodeEditor = ({ data, remappings, libraries, language, mainFile, contractName }: Props) => {
  const [ instance, setInstance ] = React.useState<Monaco | undefined>();
  const [ editor, setEditor ] = React.useState<monaco.editor.IStandaloneCodeEditor | undefined>();
  const [ index, setIndex ] = React.useState(0);
  const [ tabs, setTabs ] = React.useState([ data[index].file_path ]);
  const [ isMetaPressed, setIsMetaPressed ] = React.useState(false);

  const [ containerRect, containerNodeRef ] = useClientRect<HTMLDivElement>();

  const { colorMode } = useColorMode();
  const [ borderRadius ] = useToken('radii', 'md');
  const isMobile = useIsMobile();
  const themeColors = useThemeColors();

  const editorWidth = containerRect ? containerRect.width - (isMobile ? 0 : SIDE_BAR_WIDTH) : 0;

  const editorLanguage = (() => {
    switch (language) {
      case 'vyper':
        return 'elixir';
      case 'json':
        return 'json';
      case 'solidity':
        return 'sol';
      case 'scilla':
        return 'scilla';
      case 'stylus_rust':
        return 'rust';
      case 'geas':
        return 'geas';
      default:
        return 'javascript';
    }
  })();

  React.useEffect(() => {
    instance?.editor.setTheme(colorMode === 'light' ? 'blockscout-light' : 'blockscout-dark');
  }, [ colorMode, instance?.editor ]);

  const handleEditorDidMount = React.useCallback((editor: monaco.editor.IStandaloneCodeEditor, monaco: Monaco) => {
    setInstance(monaco);
    setEditor(editor);

    monaco.editor.defineTheme('blockscout-light', themes.light);
    monaco.editor.defineTheme('blockscout-dark', themes.dark);
    monaco.editor.setTheme(colorMode === 'light' ? 'blockscout-light' : 'blockscout-dark');

    if (editorLanguage === 'scilla') {
      monaco.languages.register({ id: editorLanguage });
      monaco.languages.setMonarchTokensProvider(editorLanguage, defScilla);
      monaco.languages.setLanguageConfiguration(editorLanguage, configScilla);
    }

    if (editorLanguage === 'geas') {
      monaco.languages.register({ id: editorLanguage });
      monaco.languages.setMonarchTokensProvider(editorLanguage, defGeas);
      monaco.languages.setLanguageConfiguration(editorLanguage, configGeas);
    }

    const loadedModels = monaco.editor.getModels();
    const loadedModelsPaths = loadedModels.map((model) => model.uri.path);
    const newModels = data.slice(1)
      .filter((file) => !loadedModelsPaths.includes(file.file_path))
      .map((file) => monaco.editor.createModel(file.source_code, editorLanguage, monaco.Uri.parse(file.file_path)));

    if (language === 'solidity') {
      loadedModels.concat(newModels)
        .forEach((model) => {
          contractName && mainFile === model.uri.path && addMainContractCodeDecoration(model, contractName, editor);
          addFileImportDecorations(model);
          libraries?.length && addExternalLibraryWarningDecoration(model, libraries);
        });
    }

    editor.addAction({
      id: 'close-tab',
      label: 'Close current tab',
      keybindings: [
        monaco.KeyMod.Alt | monaco.KeyCode.KeyW,
      ],
      contextMenuGroupId: 'navigation',
      contextMenuOrder: 1.7,
      run: function(editor) {
        const model = editor.getModel();
        const path = model?.uri.path;
        if (path) {
          handleTabClose(path, true);
        }
      },
    });
  // componentDidMount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ ]);

  const handleSelectFile = React.useCallback((index: number, lineNumber?: number) => {
    setIndex(index);
    setTabs((prev) => prev.some((item) => item === data[index].file_path) ? prev : ([ ...prev, data[index].file_path ]));
    if (lineNumber !== undefined && !Object.is(lineNumber, NaN)) {
      window.setTimeout(() => {
        editor?.revealLineInCenter(lineNumber);
      }, 0);
    }
    editor?.focus();
  }, [ data, editor ]);

  const handleTabSelect = React.useCallback((path: string) => {
    const index = data.findIndex((item) => item.file_path === path);
    if (index > -1) {
      setIndex(index);
    }
  }, [ data ]);

  const handleTabClose = React.useCallback((path: string, _isActive?: boolean) => {
    setTabs((prev) => {
      if (prev.length > 1) {
        const tabIndex = prev.findIndex((item) => item === path);
        const isActive = _isActive !== undefined ? _isActive : data[index].file_path === path;

        if (isActive) {
          const nextActiveIndex = data.findIndex((item) => item.file_path === prev[(tabIndex === 0 ? 1 : tabIndex - 1)]);
          setIndex(nextActiveIndex);
        }

        return prev.filter((item) => item !== path);
      }

      return prev;
    });
  }, [ data, index ]);

  const handleClick = React.useCallback((event: React.MouseEvent) => {
    if (!isMetaPressed && !isMobile) {
      return;
    }

    const target = event.target as HTMLSpanElement;
    const isImportLink = target.classList.contains('import-link');
    if (isImportLink) {
      const path = [
        target.previousElementSibling as HTMLSpanElement,
        target,
        target.nextElementSibling as HTMLSpanElement,
      ]
        .filter((element) => element?.classList.contains('import-link'))
        .map((element: HTMLSpanElement) => element.innerText)
        .join('');

      const fullPath = getFullPathOfImportedFile(data[index].file_path, path, remappings);
      const fileIndex = data.findIndex((file) => file.file_path === fullPath);
      if (fileIndex > -1) {
        event.stopPropagation();
        handleSelectFile(fileIndex);
      }
    }
  }, [ data, handleSelectFile, index, isMetaPressed, isMobile, remappings ]);

  const handleKeyDown = React.useCallback((event: React.KeyboardEvent) => {
    isMetaKey(event) && setIsMetaPressed(true);
  }, []);

  const handleKeyUp = React.useCallback(() => {
    setIsMetaPressed(false);
  }, []);

  const containerCss: SystemStyleObject = React.useMemo(() => ({
    '& .editor-container': {
      position: 'absolute',
      top: 0,
      left: 0,
      width: `${ editorWidth }px`,
      height: '100%',
    },
    '& .monaco-editor': {
      'border-bottom-left-radius': borderRadius,
    },
    '& .monaco-editor .overflow-guard': {
      'border-bottom-left-radius': borderRadius,
    },
    '& .monaco-editor .core-guide': {
      zIndex: 1,
    },
    // '.monaco-editor .currentFindMatch': // TODO: find a better way to style this
    '& .monaco-editor .findMatch': {
      backgroundColor: themeColors['custom.findMatchHighlightBackground'],
    },
    '& .highlight': {
      backgroundColor: themeColors['custom.findMatchHighlightBackground'],
    },
    '&&.meta-pressed .import-link:hover, &&.meta-pressed .import-link:hover + .import-link': {
      color: themeColors['custom.fileLink.hoverForeground'],
      textDecoration: 'underline',
      cursor: 'pointer',
    },
    '& .risk-warning-primary': {
      backgroundColor: themeColors['custom.riskWarning.primaryBackground'],
    },
    '& .risk-warning': {
      backgroundColor: themeColors['custom.riskWarning.background'],
    },
    '& .main-contract-header': {
      backgroundColor: themeColors['custom.mainContract.header'],
    },
    '& .main-contract-body': {
      backgroundColor: themeColors['custom.mainContract.body'],
    },
    '& .main-contract-glyph': {
      zIndex: 1,
      background: 'url(/static/contract_star.png) no-repeat center center',
      backgroundSize: '12px',
      cursor: 'pointer',
    },
  }), [ editorWidth, themeColors, borderRadius ]);

  const renderErrorScreen = React.useCallback(() => {
    return <Center bgColor={ themeColors['editor.background'] } w="100%" h="100%" borderRadius="md">Oops! Something went wrong!</Center>;
  }, [ themeColors ]);

  if (data.length === 1) {
    const css = {
      ...containerCss,
      '& .monaco-editor': {
        'border-radius': borderRadius,
      },
      '& .monaco-editor .overflow-guard': {
        'border-radius': borderRadius,
      },
    };

    return (
      <Box height={ `${ EDITOR_HEIGHT }px` } width="100%" css={ css } ref={ containerNodeRef }>
        <ErrorBoundary renderErrorScreen={ renderErrorScreen }>
          <MonacoEditor
            className="editor-container"
            language={ editorLanguage }
            path={ data[index].file_path }
            defaultValue={ data[index].source_code }
            options={ EDITOR_OPTIONS }
            onMount={ handleEditorDidMount }
            loading={ <CodeEditorLoading borderRadius="md"/> }
          />
        </ErrorBoundary>
      </Box>
    );
  }

  return (
    <Flex
      className={ isMetaPressed ? 'meta-pressed' : undefined }
      width="100%"
      height={ `${ EDITOR_HEIGHT + TABS_HEIGHT + BREADCRUMBS_HEIGHT }px` }
      position="relative"
      ref={ containerNodeRef }
      css={ containerCss }
      overflow={{ base: 'hidden', lg: 'visible' }}
      borderRadius="md"
      onClick={ handleClick }
      onKeyDown={ handleKeyDown }
      onKeyUp={ handleKeyUp }
    >
      <ErrorBoundary renderErrorScreen={ renderErrorScreen }>
        <Box flexGrow={ 1 }>
          <CodeEditorTabs
            tabs={ tabs }
            activeTab={ data[index].file_path }
            mainFile={ mainFile }
            onTabSelect={ handleTabSelect }
            onTabClose={ handleTabClose }
          />
          <CodeEditorBreadcrumbs path={ data[index].file_path }/>
          <MonacoEditor
            className="editor-container"
            height={ `${ EDITOR_HEIGHT }px` }
            language={ editorLanguage }
            path={ data[index].file_path }
            defaultValue={ data[index].source_code }
            options={ EDITOR_OPTIONS }
            onMount={ handleEditorDidMount }
            loading={ <CodeEditorLoading borderBottomLeftRadius="md"/> }
          />
        </Box>
        <CodeEditorSideBar
          data={ data }
          onFileSelect={ handleSelectFile }
          monaco={ instance }
          editor={ editor }
          selectedFile={ data[index].file_path }
          mainFile={ mainFile }
        />
      </ErrorBoundary>
    </Flex>
  );
};

export default React.memo(CodeEditor);
