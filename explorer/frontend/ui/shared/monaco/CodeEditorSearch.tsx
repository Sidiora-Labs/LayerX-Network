import type { HTMLChakraProps } from '@chakra-ui/react';
import { Box } from '@chakra-ui/react';
import type * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import React from 'react';

import type { File, Monaco, SearchResult } from './types';

import useDebounce from 'lib/hooks/useDebounce';
import { AccordionRoot } from 'toolkit/chakra/accordion';
import { Input } from 'toolkit/chakra/input';
import { InputGroup } from 'toolkit/chakra/input-group';

import CodeEditorSearchSection from './CodeEditorSearchSection';
import CoderEditorCollapseButton from './CoderEditorCollapseButton';
import useThemeColors from './utils/useThemeColors';

interface Props {
  data: Array<File>;
  monaco: Monaco | undefined;
  onFileSelect: (index: number, lineNumber?: number) => void;
  isInputStuck: boolean;
  isActive: boolean;
  setActionBarRenderer: React.Dispatch<React.SetStateAction<(() => React.JSX.Element) | undefined>>;
  defaultValue: string;
}

const CodeEditorSearch = ({ monaco, data, onFileSelect, isInputStuck, isActive, setActionBarRenderer, defaultValue }: Props) => {
  const [ searchTerm, changeSearchTerm ] = React.useState('');
  const [ searchResults, setSearchResults ] = React.useState<Array<SearchResult>>([]);
  const [ expandedSections, setExpandedSections ] = React.useState<Array<string>>([]);
  const [ isMatchCase, setMatchCase ] = React.useState(false);
  const [ isMatchWholeWord, setMatchWholeWord ] = React.useState(false);
  const [ isMatchRegex, setMatchRegex ] = React.useState(false);
  const decorations = React.useRef<Record<string, Array<string>>>({});

  const themeColors = useThemeColors();

  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const handleMatchCaseChange = React.useCallback(() => {
    setMatchCase((prev) => !prev);
  }, []);

  const handleMatchWholeWordChange = React.useCallback(() => {
    setMatchWholeWord((prev) => !prev);
  }, []);

  const handleMatchRegexChange = React.useCallback(() => {
    setMatchRegex((prev) => !prev);
  }, []);

  React.useEffect(() => {
    changeSearchTerm(defaultValue);
  }, [ defaultValue ]);

  React.useEffect(() => {
    if (!monaco) {
      return;
    }

    if (!debouncedSearchTerm) {
      setSearchResults([]);
    }

    const models = monaco.editor.getModels();
    const matches = models.map((model) => model.findMatches(debouncedSearchTerm, false, isMatchRegex, isMatchCase, isMatchWholeWord ? 'true' : null, false));

    models.forEach((model, index) => {
      const filePath = model.uri.path;
      const prevDecorations = decorations.current[filePath] || [];
      const newDecorations: Array<monaco.editor.IModelDeltaDecoration> = matches[index].map(({ range }) => ({ range, options: { className: 'highlight' } }));

      const newDecorationsIds = model.deltaDecorations(prevDecorations, newDecorations);
      decorations.current[filePath] = newDecorationsIds;
    });

    const result: Array<SearchResult> = matches
      .map((match, index) => {
        const model = models[index];
        return {
          file_path: model.uri.path,
          matches: match.map(({ range }) => ({ ...range, lineContent: model.getLineContent(range.startLineNumber) })),
        };
      })
      .filter(({ matches }) => matches.length > 0);

    setSearchResults(result.length > 0 ? result : []);
  }, [ debouncedSearchTerm, isMatchCase, isMatchRegex, isMatchWholeWord, monaco ]);

  React.useEffect(() => {
    setExpandedSections(searchResults.map((item) => item.file_path));
  }, [ searchResults ]);

  const handleSearchTermChange = React.useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    changeSearchTerm(event.target.value);
  }, []);

  const handleResultItemClick = React.useCallback((filePath: string, lineNumber: number) => {
    const fileIndex = data.findIndex((item) => item.file_path === filePath);
    if (fileIndex > -1) {
      onFileSelect(fileIndex, Number(lineNumber));
    }
  }, [ data, onFileSelect ]);

  const handleAccordionStateChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setExpandedSections(value);
  }, []);

  const handleToggleCollapseClick = React.useCallback(() => {
    if (expandedSections.length === 0) {
      setExpandedSections(searchResults.map((item) => item.file_path));
    } else {
      setExpandedSections([]);
    }
  }, [ expandedSections.length, searchResults ]);

  const renderActionBar = React.useCallback(() => {
    return (
      <CoderEditorCollapseButton
        onClick={ handleToggleCollapseClick }
        label={ expandedSections.length === 0 ? 'Expand all' : 'Collapse all' }
        isDisabled={ searchResults.length === 0 }
        isCollapsed={ expandedSections.length === 0 }
      />
    );
  }, [ expandedSections.length, handleToggleCollapseClick, searchResults.length ]);

  React.useEffect(() => {
    isActive && setActionBarRenderer(() => renderActionBar);
  }, [ isActive, renderActionBar, setActionBarRenderer ]);

  const buttonProps: HTMLChakraProps<'div'> = {
    boxSize: '20px',
    p: '1px',
    cursor: 'pointer',
    borderRadius: '3px',
    borderWidth: '1px',
    borderColor: 'transparent',
    color: 'text.primary',
  };

  const searchResultNum = (() => {
    if (!debouncedSearchTerm) {
      return null;
    }

    const totalResults = searchResults.map(({ matches }) => matches.length).reduce((result, item) => result + item, 0);

    if (!totalResults) {
      return (
        <Box px="8px" fontSize="13px" lineHeight="18px" mb="8px">
          No results found. Review your settings for configured exclusions.
        </Box>
      );
    }

    return (
      <Box px="8px" fontSize="13px" lineHeight="18px" mb="8px">
        { totalResults } result{ totalResults > 1 ? 's' : '' } in { searchResults.length } file{ searchResults.length > 1 ? 's' : '' }
      </Box>
    );
  })();

  const inputEndElement = (
    <>
      <Box
        { ...buttonProps }
        className="codicon codicon-case-sensitive"
        onClick={ handleMatchCaseChange }
        bgColor={ isMatchCase ? themeColors['custom.inputOption.activeBackground'] : 'transparent' }
        _hover={{ bgColor: isMatchCase ? themeColors['custom.inputOption.activeBackground'] : themeColors['custom.inputOption.hoverBackground'] }}
        title="Match Case"
        aria-label="Match Case"
      />
      <Box
        { ...buttonProps }
        className="codicon codicon-whole-word"
        bgColor={ isMatchWholeWord ? themeColors['custom.inputOption.activeBackground'] : 'transparent' }
        onClick={ handleMatchWholeWordChange }
        _hover={{ bgColor: isMatchWholeWord ? themeColors['custom.inputOption.activeBackground'] : themeColors['custom.inputOption.hoverBackground'] }}
        title="Match Whole Word"
        aria-label="Match Whole Word"
      />
      <Box
        { ...buttonProps }
        className="codicon codicon-regex"
        bgColor={ isMatchRegex ? themeColors['custom.inputOption.activeBackground'] : 'transparent' }
        onClick={ handleMatchRegexChange }
        _hover={{ bgColor: isMatchRegex ? themeColors['custom.inputOption.activeBackground'] : themeColors['custom.inputOption.hoverBackground'] }}
        title="Use Regular Expression"
        aria-label="Use Regular Expression"
      />
    </>
  );

  return (
    <>
      <InputGroup
        px="8px"
        position="sticky"
        top="35px"
        left="0"
        zIndex="2"
        bgColor={ themeColors['sideBar.background'] }
        pb="8px"
        boxShadow={ isInputStuck ? '0px 6px 3px 0px rgba(0, 0, 0, 0.05)' : 'none' }
        endElement={ inputEndElement }
        endElementProps={{ height: '26px', pl: '0', right: '10px', columnGap: '2px' }}
      >
        <Input
          size="xs"
          onChange={ handleSearchTermChange }
          value={ searchTerm }
          placeholder="Search"
          color={ themeColors['input.foreground'] }
          bgColor={ themeColors['input.background'] }
          borderRadius="none"
          height="26px"
          fontSize="13px"
          lineHeight="20px"
          borderWidth="1px"
          borderColor={ themeColors['input.background'] }
          py="2px"
          pl="4px"
          pr="75px"
          transitionDuration="0"
          _focus={{
            borderColor: themeColors.focusBorder,
          }}
        />

      </InputGroup>
      { searchResultNum }
      <AccordionRoot
        key={ debouncedSearchTerm }
        multiple
        value={ expandedSections }
        onValueChange={ handleAccordionStateChange }
        noAnimation
      >
        { searchResults.map((item) => <CodeEditorSearchSection key={ item.file_path } data={ item } onItemClick={ handleResultItemClick }/>) }
      </AccordionRoot>
    </>
  );
};

export default React.memo(CodeEditorSearch);
