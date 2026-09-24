import { debounce } from 'es-toolkit';
import type { FormEvent, FocusEvent } from 'react';
import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import { getRecentSearchKeywords } from 'lib/recentSearchKeywords';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import SearchBarBackdrop from 'ui/snippets/searchBar/SearchBarBackdrop';
import SearchBarInput from 'ui/snippets/searchBar/SearchBarInput';
import SearchBarRecentKeywords from 'ui/snippets/searchBar/SearchBarRecentKeywords';

type Props = {
  searchTerm: string;
  handleSubmit: (event: FormEvent<HTMLFormElement>) => void;
  handleSearchTermChange: (value: string) => void;
};

const SearchResultsInput = ({ searchTerm, handleSubmit, handleSearchTermChange }: Props) => {
  const { open, onClose, onOpen } = useDisclosure();
  const inputRef = React.useRef<HTMLFormElement>(null);
  const menuRef = React.useRef<HTMLDivElement>(null);
  const menuWidth = React.useRef<number>(0);
  const isMobile = useIsMobile();

  const recentSearchKeywords = getRecentSearchKeywords();

  const handleOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    open && onOpen();
  }, [ onOpen ]);

  const handleFocus = React.useCallback(() => {
    onOpen();
  }, [ onOpen ]);

  const handelHide = React.useCallback(() => {
    onClose();
    inputRef.current?.querySelector('input')?.blur();
  }, [ onClose ]);

  const handleBlur = React.useCallback((event: FocusEvent<HTMLFormElement>) => {
    const isFocusInMenu = menuRef.current?.contains(event.relatedTarget);
    const isFocusInInput = inputRef.current?.contains(event.relatedTarget);
    if (!isFocusInMenu && !isFocusInInput) {
      onClose();
    }
  }, [ onClose ]);

  const handleClear = React.useCallback(() => {
    handleSearchTermChange('');
    inputRef.current?.querySelector('input')?.focus();
  }, [ handleSearchTermChange ]);

  const menuPaddingX = isMobile ? 24 : 0;
  const calculateMenuWidth = React.useCallback(() => {
    menuWidth.current = (inputRef.current?.getBoundingClientRect().width || 0) - menuPaddingX;
  }, [ menuPaddingX ]);

  React.useEffect(() => {
    const inputEl = inputRef.current;
    if (!inputEl) {
      return;
    }
    calculateMenuWidth();

    const resizeHandler = debounce(calculateMenuWidth, 200);
    const resizeObserver = new ResizeObserver(resizeHandler);
    if (inputRef.current) {
      resizeObserver.observe(inputRef.current);
    }

    return function cleanup() {
      resizeObserver.unobserve(inputEl);
    };
  }, [ calculateMenuWidth ]);

  const isSuggestOpen = open && recentSearchKeywords.length > 0 && searchTerm.trim().length === 0;

  return (
    <>
      <PopoverRoot
        open={ isSuggestOpen }
        autoFocus={ false }
        onOpenChange={ handleOpenChange }
        positioning={{ offset: { mainAxis: isMobile ? 0 : 8, crossAxis: isMobile ? 12 : 0 } }}
      >
        <PopoverTrigger>
          <SearchBarInput
            ref={ inputRef }
            onChange={ handleSearchTermChange }
            onSubmit={ handleSubmit }
            onFocus={ handleFocus }
            onBlur={ handleBlur }
            onHide={ handelHide }
            onClear={ handleClear }
            value={ searchTerm }
            isSuggestOpen={ isSuggestOpen }
          />
        </PopoverTrigger>
        <PopoverContent w={ `${ menuWidth.current }px` } maxH={{ base: '300px', lg: '500px' }} overflowY="scroll" ref={ menuRef }>
          <PopoverBody py={ 6 }>
            <SearchBarRecentKeywords onClick={ handleSearchTermChange } onClear={ onClose }/>
          </PopoverBody>
        </PopoverContent>
      </PopoverRoot>
      <SearchBarBackdrop isOpen={ isSuggestOpen }/>
    </>
  );
};

export default SearchResultsInput;
