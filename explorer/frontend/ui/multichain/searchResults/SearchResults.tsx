import { chakra } from '@chakra-ui/react';
import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import SearchResultsInput from 'ui/searchResults/SearchResultsInput';
import AppErrorBoundary from 'ui/shared/AppError/AppErrorBoundary';
import * as Layout from 'ui/shared/layout/components';
import PageTitle from 'ui/shared/Page/PageTitle';
import HeaderAlert from 'ui/snippets/header/HeaderAlert';
import HeaderDesktop from 'ui/snippets/header/HeaderDesktop';
import HeaderMobile from 'ui/snippets/header/HeaderMobile';

import SearchResultTabContent from './SearchResultTabContent';
import useSearchQuery from './useSearchQuery';
import useSearchRedirect from './useSearchRedirect';
import type { QueryType } from './utils';
import { SEARCH_TABS_IDS, SEARCH_TABS_NAMES } from './utils';

const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 6,
  marginTop: -6,
  minW: { base: 'auto', lg: '1080px' },
};
const PRESERVED_PARAMS = [ 'q', 'tab', 'chain_id' ];

const SearchResults = () => {
  const isMobile = useIsMobile();

  const chainSelect = useRoutedChainSelect({
    withAllOption: true,
    persistedParams: [ 'q', 'tab' ],
  });
  const chainId = chainSelect.value?.[0];
  const { searchTerm, debouncedSearchTerm, handleSearchTermChange, handleSubmit, queries, checkRedirectQuery } = useSearchQuery({
    chainId: chainId === 'all' ? undefined : chainId,
  });
  const showContent = useSearchRedirect({ checkRedirectQuery, hasSearchTerm: debouncedSearchTerm.trim().length > 0 });

  const isLoading = Object.values(queries).some((query) => query.isPending);

  const handleNavigateToResults = React.useCallback((searchTerm: string) => {
    handleSearchTermChange(searchTerm);
  }, [ handleSearchTermChange ]);

  const renderSearchBar = React.useCallback(() => {
    return (
      <SearchResultsInput
        searchTerm={ searchTerm }
        handleSubmit={ handleSubmit }
        handleSearchTermChange={ handleSearchTermChange }
      />
    );
  }, [ handleSearchTermChange, handleSubmit, searchTerm ]);

  const totalResults = (() => {
    if (isLoading) {
      return;
    }

    const isOverflow = Object.values(queries).some((query) => query.data?.pages?.some((page) => page.next_page_params));

    const num = Object.values(queries).reduce((acc, query) => {
      return acc + (query.data?.pages[0]?.items.length ?? 0);
    }, 0);

    return { num, isOverflow };
  })();

  const chainSelectElement = (
    <ChainSelect
      value={ chainSelect.value }
      onValueChange={ chainSelect.onValueChange }
      mode="default"
      withAllOption
      w={{ base: 'full', lg: 'fit-content' }}
      mt={{ base: -3, lg: 0 }}
      mb={{ base: 6, lg: 0 }}
    />
  );

  const detailedTabs = Object.entries(SEARCH_TABS_IDS).map(([ key, value ]) => {
    const queryType = key as QueryType;

    return {
      id: value,
      title: SEARCH_TABS_NAMES[queryType],
      component: (
        <SearchResultTabContent
          queries={ queries }
          queryType={ queryType }
          isLoading={ isLoading }
          searchTerm={ debouncedSearchTerm }
          beforeContent={ isMobile ? chainSelectElement : undefined }
        />
      ),
    };
  });

  const tabs = [
    {
      id: 'all',
      title: 'All',
      component: (
        <SearchResultTabContent
          queries={ queries }
          queryType={ undefined }
          isLoading={ isLoading }
          searchTerm={ debouncedSearchTerm }
          beforeContent={ isMobile ? chainSelectElement : undefined }
        />
      ),
    },
    ...detailedTabs,
  ];

  const pageContent = !showContent ?
    <ContentLoader/> :
    (
      <>
        <PageTitle title="Search results"/>
        <Skeleton loading={ totalResults === undefined } mb={ 6 } w="fit-content">
          Found <chakra.span fontWeight={ 700 }>{ totalResults?.num }{ totalResults?.isOverflow ? '+' : '' }</chakra.span> matching results
        </Skeleton>
        <RoutedTabs
          tabs={ tabs }
          variant="secondary"
          size="sm"
          preservedParams={ PRESERVED_PARAMS }
          listProps={ isMobile ? undefined : TAB_LIST_PROPS }
          rightSlot={ isMobile ? undefined : chainSelectElement }
          stickyEnabled={ !isMobile }
        />
      </>
    );

  return (
    <>
      <HeaderMobile onGoToSearchResults={ handleNavigateToResults }/>
      <Layout.MainArea>
        <Layout.SideBar/>
        <Layout.MainColumn>
          <HeaderAlert/>
          <HeaderDesktop renderSearchBar={ renderSearchBar }/>
          <AppErrorBoundary>
            <Layout.Content>
              { pageContent }
            </Layout.Content>
          </AppErrorBoundary>
        </Layout.MainColumn>
      </Layout.MainArea>
      <Layout.Footer/>
    </>
  );
};

export default React.memo(SearchResults);
