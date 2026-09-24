import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import type { FormEvent } from 'react';
import React from 'react';

import type { Route } from 'nextjs-routes';
import { route } from 'nextjs-routes';

import * as mixpanel from 'lib/mixpanel/index';
import { getRecentSearchKeywords, saveToRecentKeywords } from 'lib/recentSearchKeywords';
import { Button } from 'toolkit/chakra/button';
import {
  DrawerRoot,
  DrawerTrigger,
  DrawerContent,
  DrawerHeader,
  DrawerTitle,
  DrawerCloseTrigger,
  DrawerBody,
  DrawerFooter,
} from 'toolkit/chakra/drawer';
import { Link } from 'toolkit/chakra/link';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import IconSvg from 'ui/shared/IconSvg';
import SearchBarInput from 'ui/snippets/searchBar/SearchBarInput';

import SearchBarRecentKeywords from './SearchBarRecentKeywords';
import SearchBarSuggest from './SearchBarSuggest/SearchBarSuggest';
import useQuickSearchQuery from './useQuickSearchQuery';

type Props = {
  isHeroBanner?: boolean;
  onGoToSearchResults?: (searchTerm: string) => void;
};

const SearchBarMobile = ({ isHeroBanner, onGoToSearchResults }: Props) => {
  const inputRef = React.useRef<HTMLFormElement>(null);
  const router = useRouter();

  const { open, onOpen, onClose, onOpenChange } = useDisclosure();
  const { searchTerm, debouncedSearchTerm, handleSearchTermChange, query, zetaChainCCTXQuery, externalSearchItem } = useQuickSearchQuery();
  const recentSearchKeywords = getRecentSearchKeywords();

  const navigateToResults = React.useCallback((redirect: boolean) => {
    if (searchTerm) {
      const resultRoute: Route = { pathname: '/search-results', query: { q: searchTerm, redirect: redirect ? 'true' : 'false' } };
      const url = route(resultRoute);
      mixpanel.logEvent(mixpanel.EventTypes.SEARCH_QUERY, {
        'Search query': searchTerm,
        'Source page type': mixpanel.getPageType(router.pathname),
        'Result URL': url,
      });
      saveToRecentKeywords(searchTerm);
      router.push(resultRoute, undefined, { shallow: true });
      onGoToSearchResults?.(searchTerm);
      onClose();
    }
  }, [ searchTerm, router, onGoToSearchResults, onClose ]);

  const handleSubmit = React.useCallback((event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    navigateToResults(true);
  }, [ navigateToResults ]);

  const handleViewAllResultsClick = React.useCallback(() => {
    navigateToResults(false);
  }, [ navigateToResults ]);

  const onTriggerClick = React.useCallback((event: React.MouseEvent) => {
    onOpen();
    event.preventDefault();
    event.stopPropagation();
  }, [ onOpen ]);

  const handleItemClick = React.useCallback((event: React.MouseEvent<HTMLAnchorElement>) => {
    onClose();
    mixpanel.logEvent(mixpanel.EventTypes.SEARCH_QUERY, {
      'Search query': searchTerm,
      'Source page type': mixpanel.getPageType(router.pathname),
      'Result URL': event.currentTarget.href,
    });
    saveToRecentKeywords(searchTerm);
  }, [ router.pathname, searchTerm, onClose ]);

  const handleClear = React.useCallback(() => {
    handleSearchTermChange('');
    inputRef.current?.querySelector('input')?.focus();
  }, [ handleSearchTermChange ]);

  const handleOverlayClick = React.useCallback((event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    onTriggerClick(event);
  }, [ onTriggerClick ]);

  let trigger: React.ReactNode | null = null;
  if (isHeroBanner) {
    trigger = (
      <Box position="relative" width="100%">
        <SearchBarInput
          isHeroBanner={ isHeroBanner }
          readOnly={ true }
        />
        <Box
          onClick={ handleOverlayClick }
          aria-label="Search"
          cursor="pointer"
          zIndex={ 1 }
          position="absolute"
          top={ 0 }
          left={ 0 }
          right={ 0 }
          bottom={ 0 }
        />
      </Box>
    );
  } else {
    trigger = (
      <Button
        variant="header"
        flexShrink={ 0 }
        p={ 0 }
      >
        <IconSvg
          name="search"
          boxSize={ 6 }
          flexShrink={ 0 }
        />
      </Button>
    );
  }

  return (
    <DrawerRoot placement="bottom" open={ open } onOpenChange={ onOpenChange } unmountOnExit={ false } lazyMount={ true }>
      <DrawerTrigger asChild>
        { trigger }
      </DrawerTrigger>
      <DrawerContent h="75vh" overflowY="hidden">
        <DrawerHeader>
          <DrawerTitle>Search</DrawerTitle>
          <DrawerCloseTrigger/>
        </DrawerHeader>
        <DrawerBody overflow="hidden" display="flex" flexDirection="column">
          <SearchBarInput
            ref={ inputRef }
            onChange={ handleSearchTermChange }
            onClear={ handleClear }
            onSubmit={ handleSubmit }
            value={ searchTerm }
            mb={ 5 }
          />
          { searchTerm.trim().length === 0 && recentSearchKeywords.length > 0 && (
            <SearchBarRecentKeywords onClick={ handleSearchTermChange }/>
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
        </DrawerBody>
        { (query.data && query.data?.length > 0) && (
          <DrawerFooter
            borderTop="1px solid"
            borderColor="border.divider"
            pt={ 3 }
            px={ 5 }
            pb={ 5 }
            justifyContent="center"
          >
            <Link
              onClick={ handleViewAllResultsClick }
              textStyle="sm"
            >
              View all results
            </Link>
          </DrawerFooter>
        ) }
      </DrawerContent>
    </DrawerRoot>
  );
};

export default SearchBarMobile;
