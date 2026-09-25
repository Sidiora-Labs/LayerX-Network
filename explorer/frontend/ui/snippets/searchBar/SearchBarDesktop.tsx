import { useClickAway } from '@uidotdev/usehooks';
import { debounce } from 'es-toolkit';
import { useRouter } from 'next/router';
import type { FormEvent } from 'react';
import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import * as mixpanel from 'lib/mixpanel/index';
import { getRecentSearchKeywords, saveToRecentKeywords } from 'lib/recentSearchKeywords';
import { Link } from 'toolkit/chakra/link';
import { PopoverBody, PopoverContent, PopoverFooter, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';

import SearchBarBackdrop from './SearchBarBackdrop';
import SearchBarInput from './SearchBarInput';
import SearchBarRecentKeywords from './SearchBarRecentKeywords';
import SearchBarSuggest from './SearchBarSuggest/SearchBarSuggest';
import useSearchWithClusters from './useSearchWithClusters';
import { getSearchRedirectRoute, useSearchRedirect, useSearchRequestGuard } from './utils';

const paxeerXFeature = config.features.paxeerXLists;

type Props = {
  isHeroBanner?: boolean;
};

const SearchBarDesktop = ({ isHeroBanner }: Props) => {
  const inputRef = React.useRef<HTMLFormElement>(null);
  const menuWidth = React.useRef<number>(0);

  const { open, onClose, onOpen } = useDisclosure();
  const isMobile = useIsMobile();
  const router = useRouter();

  const recentSearchKeywords = getRecentSearchKeywords();

  const { searchTerm, debouncedSearchTerm, handleSearchTermChange, query, zetaChainCCTXQuery, externalSearchItem } = useSearchWithClusters();
  const resolveSearchRoute = useSearchRedirect();
  const searchGuard = useSearchRequestGuard();
  const searchTermRef = React.useRef(searchTerm);

  React.useEffect(() => {
    searchTermRef.current = searchTerm;
  }, [ searchTerm ]);

  const handleTermChange = React.useCallback((value: string) => {
    searchGuard.discard();
    searchTermRef.current = value;
    handleSearchTermChange(value);
  }, [ searchGuard, handleSearchTermChange ]);

  const navigateToResults = React.useCallback(async(redirect: boolean) => {
    if (searchTerm) {
      const isCurrentRequest = searchGuard.start(searchTerm);
      const resultRoute = paxeerXFeature.isEnabled ?
        await resolveSearchRoute(searchTerm, redirect) :
        getSearchRedirectRoute(searchTerm, redirect);

      if (!isCurrentRequest(searchTermRef.current)) {
        return;
      }

      const url = route(resultRoute);
      mixpanel.logEvent(mixpanel.EventTypes.SEARCH_QUERY, {
        'Search query': searchTerm,
        'Source page type': mixpanel.getPageType(router.pathname),
        'Result URL': url,
      });
      saveToRecentKeywords(searchTerm);
      router.push(resultRoute, undefined, { shallow: resultRoute.pathname === '/search-results' });
    }
  }, [ searchTerm, router, resolveSearchRoute, searchGuard ]);

  const handleSubmit = React.useCallback((event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    navigateToResults(true);
  }, [ navigateToResults ]);

  const handleViewAllResultsClick = React.useCallback(() => {
    navigateToResults(false);
  }, [ navigateToResults ]);

  const handleFocus = React.useCallback(() => {
    onOpen();
  }, [ onOpen ]);

  const handelHide = React.useCallback(() => {
    searchGuard.discard();
    onClose();
    inputRef.current?.querySelector('input')?.blur();
  }, [ searchGuard, onClose ]);

  const handleOutsideClick = React.useCallback((event: Event) => {
    const isFocusInInput = inputRef.current?.contains(event.target as Node);

    if (!isFocusInInput) {
      handelHide();
    }
  }, [ handelHide ]);

  const menuRef = useClickAway<HTMLDivElement>(handleOutsideClick);

  const handleOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    open && onOpen();
  }, [ onOpen ]);

  const handleClear = React.useCallback(() => {
    handleTermChange('');
    inputRef.current?.querySelector('input')?.focus();
  }, [ handleTermChange ]);

  const handleItemClick = React.useCallback((event: React.MouseEvent<HTMLAnchorElement>) => {
    searchGuard.discard();
    mixpanel.logEvent(mixpanel.EventTypes.SEARCH_QUERY, {
      'Search query': searchTerm,
      'Source page type': mixpanel.getPageType(router.pathname),
      'Result URL': event.currentTarget.href,
    });
    saveToRecentKeywords(searchTerm);
    onClose();
  }, [ searchGuard, router.pathname, searchTerm, onClose ]);

  const handleBlur = React.useCallback((event: React.FocusEvent<HTMLFormElement>) => {
    const isFocusInMenu = menuRef.current?.contains(event.relatedTarget);
    const isFocusInInput = inputRef.current?.contains(event.relatedTarget);
    if (!isFocusInMenu && !isFocusInInput) {
      onClose();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ onClose ]);

  const menuPaddingX = isMobile && !isHeroBanner ? 24 : 0;
  const calculateMenuWidth = React.useCallback(() => {
    menuWidth.current = (inputRef.current?.getBoundingClientRect().width || 0) - menuPaddingX;
  }, [ menuPaddingX ]);

  // clear input on page change
  React.useEffect(() => {
    handleTermChange('');
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ router.asPath?.split('?')?.[0] ]);

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

  const showAllResultsLink = searchTerm.trim().length > 0 && (
    (query.data && query.data.length >= 50) ||
    (zetaChainCCTXQuery.data && zetaChainCCTXQuery.data?.items.length > 10)
  );

  return (
    <>
      <PopoverRoot
        open={ open && (searchTerm.trim().length > 0 || recentSearchKeywords.length > 0) }
        autoFocus={ false }
        onOpenChange={ handleOpenChange }
        positioning={{ offset: isMobile && !isHeroBanner ? { mainAxis: 0, crossAxis: 12 } : { mainAxis: 8, crossAxis: 0 } }}
        lazyMount
        closeOnInteractOutside={ false }
      >
        <PopoverTrigger asChild w="100%">
          <SearchBarInput
            ref={ inputRef }
            onChange={ handleTermChange }
            onSubmit={ handleSubmit }
            onFocus={ handleFocus }
            onHide={ handelHide }
            onBlur={ handleBlur }
            onClear={ handleClear }
            isHeroBanner={ isHeroBanner }
            value={ searchTerm }
            isSuggestOpen={ open }
          />
        </PopoverTrigger>
        <PopoverContent
          maxW={{ base: 'calc(100vw - 8px)', lg: 'unset' }}
          w={ `${ menuWidth.current }px` }
          ref={ menuRef }
          overflow="hidden"
          zIndex="modal"
        >
          <PopoverBody
            px={ 4 }
            color="chakra-body-text"
            maxH="50vh"
            display="flex"
            flexDirection="column"
            overflowY="hidden"
          >
            { searchTerm.trim().length === 0 && recentSearchKeywords.length > 0 && (
              <SearchBarRecentKeywords onClick={ handleTermChange } onClear={ onClose }/>
            ) }
            { searchTerm.trim().length > 0 && (
              <SearchBarSuggest
                query={ query }
                searchTerm={ debouncedSearchTerm }
                onItemClick={ handleItemClick }
                zetaChainCCTXQuery={ zetaChainCCTXQuery }
                externalSearchItem={ externalSearchItem }
              />
            ) }
          </PopoverBody>
          { showAllResultsLink && (
            <PopoverFooter pt={ 2 } borderTopWidth={ 1 } borderColor="border.divider">
              <Link
                textStyle="sm"
                onClick={ handleViewAllResultsClick }
              >
                View all results
              </Link>
            </PopoverFooter>
          ) }
        </PopoverContent>
      </PopoverRoot>
      <SearchBarBackdrop isOpen={ open }/>
    </>
  );
};

export default SearchBarDesktop;
