import { Box, Text } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React, { useCallback } from 'react';

import { detectInputType } from 'lib/clusters/detectInputType';
import {
  shouldShowDirectoryView,
  transformLeaderboardData,
  transformFullDirectoryData,
  applyDirectoryPagination,
  calculateHasNextPage,
  getCurrentDataLength,
} from 'lib/clusters/pageUtils';
import type { ViewMode } from 'lib/clusters/pageUtils';
import { useClusterPagination } from 'lib/clusters/useClusterPagination';
import { useClustersData } from 'lib/clusters/useClustersData';
import { useClusterSearch } from 'lib/clusters/useClusterSearch';
import getQueryParamString from 'lib/router/getQueryParamString';
import { useQueryParams } from 'lib/router/useQueryParams';
import { Link } from 'toolkit/chakra/link';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';

import ClustersActionBar from './ClustersActionBar';
import ClustersDirectoryListItem from './ClustersDirectoryListItem';
import ClustersDirectoryTable from './ClustersDirectoryTable';
import ClustersLeaderboardListItem from './ClustersLeaderboardListItem';
import ClustersLeaderboardTable from './ClustersLeaderboardTable';

const Clusters = () => {
  const router = useRouter();
  const { updateQuery } = useQueryParams();

  const { searchTerm, debouncedSearchTerm } = useClusterSearch();
  const viewMode = (getQueryParamString(router.query.view) as ViewMode) || 'directory';
  const page = parseInt(getQueryParamString(router.query.page) || '1', 10);

  const inputType = React.useMemo(() => {
    if (!debouncedSearchTerm) return 'cluster_name';
    return detectInputType(debouncedSearchTerm);
  }, [ debouncedSearchTerm ]);

  const { data, clusterDetails, isError, isLoading, isClusterDetailsLoading } = useClustersData(debouncedSearchTerm, viewMode, page);

  const showDirectoryView = shouldShowDirectoryView(viewMode, Boolean(debouncedSearchTerm));

  const leaderboardData = transformLeaderboardData(data, showDirectoryView);

  const fullDirectoryData = transformFullDirectoryData(data, clusterDetails, inputType, showDirectoryView);

  const limit = 50;

  const directoryData = applyDirectoryPagination(fullDirectoryData, inputType, page, limit);

  const currentDataLength = getCurrentDataLength(showDirectoryView, directoryData.length, leaderboardData.length);

  const hasNextPage = calculateHasNextPage(
    data,
    leaderboardData.length,
    fullDirectoryData.length,
    showDirectoryView,
    inputType,
    page,
    Boolean(debouncedSearchTerm),
    limit,
  );

  const { pagination } = useClusterPagination(hasNextPage, isLoading);

  const handleViewModeChange = useCallback((newViewMode: ViewMode) => {
    updateQuery({
      view: newViewMode === 'directory' ? undefined : newViewMode,
      page: undefined,
    });
  }, [ updateQuery ]);

  const handleSearchChange = useCallback((value: string) => {
    updateQuery({
      q: value || undefined,
      page: undefined,
    });
  }, [ updateQuery ]);

  const hasActiveFilters = Boolean(debouncedSearchTerm);

  const content = (
    <>
      <Box hideFrom="lg">
        { showDirectoryView ? (
          directoryData.map((item, index) => (
            <ClustersDirectoryListItem
              key={ `${ item.name }-${ index }${ isLoading ? '-loading' : '' }` }
              item={ item }
              isLoading={ isLoading }
              isClusterDetailsLoading={ isClusterDetailsLoading && inputType === 'address' }
            />
          ))
        ) : (
          leaderboardData.map((item, index) => (
            <ClustersLeaderboardListItem
              key={ `${ item.name }-${ index }${ isLoading ? '-loading' : '' }` }
              item={ item }
              isLoading={ isLoading }
            />
          ))
        ) }
      </Box>
      <Box hideBelow="lg">
        { showDirectoryView ? (
          <ClustersDirectoryTable
            data={ directoryData }
            isLoading={ isLoading }
            isClusterDetailsLoading={ isClusterDetailsLoading && inputType === 'address' }
            top={ ACTION_BAR_HEIGHT_DESKTOP }
          />
        ) : (
          <ClustersLeaderboardTable
            data={ leaderboardData }
            isLoading={ isLoading }
            top={ ACTION_BAR_HEIGHT_DESKTOP }
          />
        ) }
      </Box>
    </>
  );

  const actionBar = (
    <ClustersActionBar
      isLoading={ isLoading }
      searchTerm={ searchTerm }
      onSearchChange={ handleSearchChange }
      viewMode={ viewMode }
      onViewModeChange={ handleViewModeChange }
      pagination={ pagination }
    />
  );

  return (
    <>
      <Text mb={ 6 } textStyle={{ base: 'sm', lg: 'md' }}>
        <Link href="https://clusters.xyz/?utm_source=blockscout" external noIcon>Clusters</Link>{ ' ' }
        is a cross-chain name service for managing addresses on multiple blockchains using a universal naming directory.
      </Text>
      <DataListDisplay
        isError={ isError }
        itemsNum={ currentDataLength }
        hasActiveFilters={ hasActiveFilters }
        emptyStateProps={{
          term: 'cluster',
        }}
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default Clusters;
