import { HStack } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { INTERCHAIN_MESSAGE } from 'stubs/interchainIndexer';
import { generateListStub } from 'stubs/utils';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import AddressTxsCrossChain from 'ui/crossChain/address/AddressTxsCrossChain';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TxsWithAPISorting from 'ui/txs/TxsWithAPISorting';

import AddressCsvExportLink from './AddressCsvExportLink';
import AddressTxsFilter from './AddressTxsFilter';
import useAddressTxsQuery from './useAddressTxsQuery';

export const ADDRESS_TXS_TAB_IDS = [ 'txs_local' as const, 'txs_cross_chain' as const ];
const TAB_LIST_PROPS = {
  marginBottom: 0,
  pt: 6,
  pb: 3,
  marginTop: -6,
};

interface Props {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
}

const AddressTxs = ({ shouldRender = true, isQueryEnabled = true }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();
  const isMobile = useIsMobile();
  const hash = getQueryParamString(router.query.hash);
  const tab = getQueryParamString(router.query.tab) as typeof ADDRESS_TXS_TAB_IDS[number] | 'txs' | undefined;

  const isLocalTab = tab === 'txs_local' || tab === 'txs';

  const localQuery = useAddressTxsQuery({
    addressHash: hash,
    enabled: isQueryEnabled && isLocalTab,
  });

  const crossChainQuery = useQueryWithPages({
    resourceName: 'interchainIndexer:address_messages',
    pathParams: { hash },
    options: {
      placeholderData: generateListStub<'interchainIndexer:address_messages'>(INTERCHAIN_MESSAGE, 50, { next_page_params: undefined }),
      enabled: isQueryEnabled && !isLocalTab,
    },
  });

  const handleTabValueChange = React.useCallback(({ value }: { value: string }) => {
    if (value === 'txs_local') {
      localQuery.setFilterValue(undefined);
    }
  }, [ localQuery ]);

  const txsLocalFilter = isLocalTab ? (
    <AddressTxsFilter
      initialValue={ localQuery.initialFilterValue }
      onFilterChange={ localQuery.onFilterChange }
      hasActiveFilter={ Boolean(localQuery.filterValue) }
      isLoading={ localQuery.query.pagination.isLoading }
    />
  ) : null;

  if (!isMounted || !shouldRender) {
    return null;
  }

  const tabs = [
    {
      id: [ 'txs_local', 'txs' ],
      title: 'Txns',
      component: (
        <TxsWithAPISorting
          filter={ txsLocalFilter }
          filterValue={ localQuery.filterValue }
          query={ localQuery.query }
          currentAddress={ hash }
          enableTimeIncrement
          socketType="address_txs"
          top={ ACTION_BAR_HEIGHT_DESKTOP }
          sorting={ localQuery.sort }
          setSort={ localQuery.setSort }
          showBlockInfo
          showTableViewButton
        />
      ),
    },
    config.features.crossChainTxs.isEnabled && {
      id: 'txs_cross_chain',
      title: 'Cross-chain txns',
      component: (
        <AddressTxsCrossChain
          pagination={ crossChainQuery.pagination }
          items={ crossChainQuery.data?.items }
          isLoading={ crossChainQuery.isPlaceholderData }
          isError={ crossChainQuery.isError }
          currentAddress={ hash }
        />
      ),
    },
  ].filter(Boolean);

  const rightSlot = (() => {
    if (isLocalTab) {
      if (isMobile) {
        return null;
      }

      return (
        <>
          <HStack gap={ 2 }>
            { txsLocalFilter }
          </HStack>
          <HStack gap={ 6 }>
            <AddressCsvExportLink
              address={ hash }
              params={{ type: 'transactions', filterType: 'address', filterValue: localQuery.filterValue }}
              isLoading={ localQuery.query.pagination.isLoading }
            />
            <Pagination { ...localQuery.query.pagination }/>
          </HStack>
        </>
      );
    }

    if (config.features.crossChainTxs.isEnabled) {
      if (isMobile) {
        return null;
      }
      return <Pagination { ...crossChainQuery.pagination } ml="auto"/>;
    }

    return null;
  })();

  const rightSlotProps = (() => {
    return {
      display: 'flex',
      justifyContent: { base: 'flex-end', lg: 'space-between' },
      ml: tabs.length > 1 ? { base: 0, lg: 4 } : 0,
      widthAllocation: 'available' as const,
    };
  })();

  return (
    <RoutedTabs
      variant="secondary"
      size="sm"
      tabs={ tabs }
      onValueChange={ handleTabValueChange }
      defaultTabId="txs_local"
      rightSlot={ rightSlot }
      rightSlotProps={ rightSlotProps }
      listProps={ isMobile ? undefined : TAB_LIST_PROPS }
      stickyEnabled={ !isMobile }
    />
  );
};

export default React.memo(AddressTxs);
