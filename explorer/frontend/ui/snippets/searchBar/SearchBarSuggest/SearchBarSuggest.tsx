import { Box, Flex, Text } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import { debounce } from 'es-toolkit';
import React from 'react';

import type { ListCctxsResponse } from '@blockscout/zetachain-cctx-types';
import type { QuickSearchResultItem } from 'types/client/search';

import config from 'configs/app';
import multichainConfig from 'configs/multichain';
import type { ResourceError } from 'lib/api/resources';
import { useSettingsContext } from 'lib/contexts/settings';
import useIsMobile from 'lib/hooks/useIsMobile';
import type { ExternalSearchItem as ExternalSearchItemType } from 'lib/search/externalSearch';
import AdaptiveTabs from 'toolkit/components/AdaptiveTabs/AdaptiveTabs';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import * as regexp from 'toolkit/utils/regexp';
import useMarketplaceApps from 'ui/marketplace/useMarketplaceApps';
import TextAd from 'ui/shared/ad/TextAd';
import ExternalSearchItem from 'ui/shared/search/ExternalSearchItem';
import type { ApiCategory, Category, ItemsCategoriesMap } from 'ui/shared/search/utils';
import { getItemCategory, searchCategories } from 'ui/shared/search/utils';

import SearchBarSuggestApp from './SearchBarSuggestApp';
import SearchBarSuggestBlockCountdown from './SearchBarSuggestBlockCountdown';
import SearchBarSuggestItem from './SearchBarSuggestItem';
import SearchBarSuggestZetaChainCCTX from './SearchBarSuggestZetaChainCCTX';

const TABS_HEIGHT = 72;

interface Props {
  query: UseQueryResult<Array<QuickSearchResultItem>, ResourceError<unknown>>;
  zetaChainCCTXQuery: UseQueryResult<ListCctxsResponse, ResourceError<unknown>>;
  externalSearchItem: ExternalSearchItemType;
  searchTerm: string;
  onItemClick: (event: React.MouseEvent<HTMLAnchorElement>) => void;
}

const SearchBarSuggest = ({ query, zetaChainCCTXQuery, externalSearchItem, searchTerm, onItemClick }: Props) => {
  const isMobile = useIsMobile();

  const scrollContainerRef = React.useRef<HTMLDivElement>(null);
  const marketplaceApps = useMarketplaceApps(searchTerm);
  const settingsContext = useSettingsContext();

  const categoriesRefs = React.useRef<Array<HTMLParagraphElement>>([]);

  const [ currentTab, setCurrentTab ] = React.useState<Category | undefined>(undefined);

  const handleScroll = React.useCallback(() => {
    const container = scrollContainerRef.current;
    if (!container || (!query.data?.length && !zetaChainCCTXQuery.data?.items.length)) {
      return;
    }
    const topLimit = container.getBoundingClientRect().y + TABS_HEIGHT;
    if (categoriesRefs.current[categoriesRefs.current.length - 1].getBoundingClientRect().y <= topLimit) {
      const lastCategory = categoriesRefs.current[categoriesRefs.current.length - 1];
      const lastCategoryId = lastCategory.getAttribute('data-id');
      if (lastCategoryId) {
        setCurrentTab(lastCategoryId as Category);
      }
      return;
    }
    for (let i = 0; i < categoriesRefs.current.length - 1; i++) {
      if (categoriesRefs.current[i]?.getBoundingClientRect().y <= topLimit && categoriesRefs.current[i + 1]?.getBoundingClientRect().y > topLimit) {
        const currentCategory = categoriesRefs.current[i];
        const currentCategoryId = currentCategory.getAttribute('data-id');
        if (currentCategoryId) {
          setCurrentTab(currentCategoryId as Category);
        }
        break;
      }
    }
  }, [ query.data, zetaChainCCTXQuery.data ]);

  React.useEffect(() => {
    const container = scrollContainerRef.current;
    const debouncedHandleScroll = debounce(handleScroll, 300);
    if (container) {
      container.addEventListener('scroll', debouncedHandleScroll);
    }
    return () => {
      if (container) {
        container.removeEventListener('scroll', debouncedHandleScroll);
      }
    };
  }, [ handleScroll ]);

  const itemsGroups = React.useMemo(() => {
    if (!query.data && !zetaChainCCTXQuery.data?.items.length && !marketplaceApps.displayedApps) {
      return {};
    }

    const map: Partial<ItemsCategoriesMap> = {};

    query.data?.forEach(item => {
      const cat = getItemCategory(item) as ApiCategory;
      if (cat) {
        if (cat in map) {
          map[cat]?.push(item);
        } else {
          map[cat] = [ item ];
        }
      }
    });

    if (marketplaceApps.displayedApps.length) {
      map.app = marketplaceApps.displayedApps;
    }

    if (zetaChainCCTXQuery.data?.items.length) {
      map.zetaChainCCTX = zetaChainCCTXQuery.data.items;
    }

    if (Object.keys(map).length > 0 && !map.block && regexp.BLOCK_HEIGHT.test(searchTerm) && !multichainConfig()) {
      map['block'] = [ {
        type: 'block',
        block_type: 'block',
        block_number: searchTerm,
        block_hash: '',
        timestamp: undefined,
      } ];
    }

    return map;
  }, [ query.data, marketplaceApps.displayedApps, searchTerm, zetaChainCCTXQuery.data?.items ]);

  React.useEffect(() => {
    categoriesRefs.current = Array(Object.keys(itemsGroups).length).fill('').map((_, i) => categoriesRefs.current[i] || React.createRef());
    const resultCategories = searchCategories.filter(cat => itemsGroups[cat.id]);
    setCurrentTab(resultCategories[0]?.id);
  }, [ itemsGroups ]);

  const handleTabsValueChange = React.useCallback(({ value }: { value: string }) => {
    setCurrentTab(value as Category);
    const container = scrollContainerRef.current;
    const targetElement = document.querySelector(`[data-scroll-target="cat_${ value }"]`);

    if (container && targetElement) {
      const containerRect = container.getBoundingClientRect();
      const targetRect = targetElement.getBoundingClientRect();
      const scrollTop = targetRect.top - containerRect.top + container.scrollTop;

      container.scrollTo({
        top: scrollTop,
        behavior: 'smooth',
      });
    }
  }, []);

  const categoryTabs = React.useMemo(() => {
    return searchCategories.filter(cat => itemsGroups[cat.id]).map(cat => ({
      id: cat.id,
      value: cat.id,
      title: isMobile ? cat.tabTitle : cat.title,
      component: null,
    }));
  }, [ itemsGroups, isMobile ]);

  const content = (() => {
    if (query.isPending || marketplaceApps.isPlaceholderData || (config.features.zetachain.isEnabled && zetaChainCCTXQuery.isPending)) {
      return <ContentLoader text="We are searching, please wait... " fontSize="sm" maxW="250px"/>;
    }

    if (query.isError) {
      return <Text>Something went wrong. Try refreshing the page or come back later.</Text>;
    }

    const resultCategories = searchCategories.filter(cat => itemsGroups[cat.id]);

    if (resultCategories.length === 0 && !externalSearchItem) {
      if (regexp.BLOCK_HEIGHT.test(searchTerm)) {
        return <SearchBarSuggestBlockCountdown blockHeight={ searchTerm } isMultichain={ Boolean(multichainConfig()) } onClick={ onItemClick }/>;
      }

      return <Text>No results found.</Text>;
    }

    return (
      <>
        { resultCategories.length > 1 && (
          <AdaptiveTabs
            tabs={ categoryTabs }
            onValueChange={ handleTabsValueChange }
            defaultValue={ currentTab }
            variant="secondary"
            size="sm"
            pb={ 5 }
            w="100%"
            overflowX="hidden"
            minH="52px"
            h="52px"
            listProps={{
              overflowX: 'auto',
              mb: 0,
              mt: 0,
              bgColor: 'dialog.bg',
            }}
          />
        ) }
        <Flex flexDirection="column" overflowY="auto" ref={ scrollContainerRef }>
          { resultCategories.map((cat, index) => {
            return (
              <Box key={ cat.id } data-scroll-target={ `cat_${ cat.id }` }>
                <Text
                  textStyle="sm"
                  fontWeight={ 600 }
                  color="text.secondary"
                  mt={ index === 0 ? 1 : 6 }
                  mb={{ base: 2, lg: 3 }}
                  ref={ (el: HTMLParagraphElement) => {
                    categoriesRefs.current[index] = el;
                  } }
                  data-id={ cat.id }
                >
                  { cat.title }
                </Text>
                { cat.id !== 'app' && cat.id !== 'zetaChainCCTX' && itemsGroups[cat.id]?.map((item, index) => (
                  <SearchBarSuggestItem
                    key={ index }
                    data={ item }
                    isMobile={ isMobile }
                    searchTerm={ searchTerm }
                    onClick={ onItemClick }
                    addressFormat={ settingsContext?.addressFormat }
                  />
                )) }
                { cat.id === 'app' && itemsGroups[cat.id]?.map((item, index) =>
                  <SearchBarSuggestApp key={ index } data={ item } isMobile={ isMobile } searchTerm={ searchTerm } onClick={ onItemClick }/>,
                ) }
                { cat.id === 'zetaChainCCTX' && itemsGroups[cat.id]?.map((item, index) =>
                  <SearchBarSuggestZetaChainCCTX key={ index } data={ item } isMobile={ isMobile } searchTerm={ searchTerm } onClick={ onItemClick }/>,
                ) }
              </Box>
            );
          }) }
          { externalSearchItem && <ExternalSearchItem item={ externalSearchItem }/> }
        </Flex>
      </>
    );
  })();

  return (
    <>
      { !isMobile && (
        <Box pb={ 4 } mb={ 5 } borderColor="border.divider" borderBottomWidth="1px" _empty={{ display: 'none' }}>
          <TextAd textStyle={{ lg: 'sm' }}/>
        </Box>
      ) }
      { content }
    </>
  );
};

export default SearchBarSuggest;
