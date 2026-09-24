import { useRouter } from 'next/router';
import React from 'react';

import type { TokenType } from 'types/api/token';
import type { TokenTransfer } from 'types/api/tokenTransfer';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { getTokenTransfersStub } from 'stubs/token';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import TokenTransferFilter from 'ui/shared/TokenTransfer/TokenTransferFilter';
import { getTokenFilterValue } from 'ui/tokens/utils';
import TxPendingAlert from 'ui/tx/TxPendingAlert';
import TxSocketAlert from 'ui/tx/TxSocketAlert';

import TxTokenTransferCrossChain from './tokenTransfers/TxTokenTransferCrossChain';
import TxTokenTransferLocal from './tokenTransfers/TxTokenTransferLocal';
import useTxCrossChainTransfersQuery from './useTxCrossChainTransfersQuery';
import type { TxQuery } from './useTxQuery';

interface Props {
  txQuery: TxQuery;
  tokenTransferFilter?: (data: TokenTransfer) => boolean;
  noCrossChain?: boolean;
}

const TxTokenTransfer = ({ txQuery, tokenTransferFilter, noCrossChain }: Props) => {
  const router = useRouter();
  const multichainContext = useMultichainContext();
  const isMobile = useIsMobile();
  const tab = getQueryParamString(router.query.tab);

  const areQueriesEnabled = !txQuery.isPlaceholderData && Boolean(txQuery.data?.status && txQuery.data?.hash);

  const [ typeFilter, setTypeFilter ] = React.useState<Array<TokenType>>(
    getTokenFilterValue(router.query.type, multichainContext?.chain?.app_config) || [],
  );

  const localQuery = useQueryWithPages({
    resourceName: 'general:tx_token_transfers',
    pathParams: { hash: txQuery.data?.hash.toString() },
    options: {
      enabled: areQueriesEnabled,
      placeholderData: getTokenTransfersStub(),
    },
    filters: { type: typeFilter },
  });

  const crossChainQuery = useTxCrossChainTransfersQuery({
    hash: String(txQuery.data?.hash),
    enabled: areQueriesEnabled && !noCrossChain,
  });

  const hasCrossChainTab =
    config.features.crossChainTxs.isEnabled &&
    Boolean(!crossChainQuery.isPlaceholderData && crossChainQuery.data?.items.length) &&
    !noCrossChain;
  const isLocalTab = tab === 'token_transfers' || (tab !== 'token_transfers_cross_chain' && !hasCrossChainTab);
  const isTabsLoading = useIsInitialLoading(
    localQuery.isPlaceholderData ||
    (config.features.crossChainTxs.isEnabled && crossChainQuery.isPlaceholderData && !noCrossChain),
  );

  const tabs = [
    {
      id: 'token_transfers',
      title: 'Transfers',
      component: (
        <TxTokenTransferLocal
          txQuery={ txQuery }
          tokenTransferFilter={ tokenTransferFilter }
          tokenTransferQuery={ localQuery }
          numActiveFilters={ typeFilter.length }
        />
      ),
    },
    hasCrossChainTab && {
      id: 'token_transfers_cross_chain',
      title: 'Cross-chain',
      component: (
        <TxTokenTransferCrossChain
          txQuery={ txQuery }
          crossChainQuery={ crossChainQuery }
          isLoading={ isTabsLoading }
          tableTop={ ACTION_BAR_HEIGHT_DESKTOP }
        />
      ),
    },
  ].filter(Boolean);

  const handleTypeFilterChange = React.useCallback((nextValue: Array<TokenType>) => {
    localQuery.onFilterChange({ type: nextValue });
    setTypeFilter(nextValue);
  }, [ localQuery ]);

  const rightSlotProps = React.useMemo(() => {
    if (tabs.length === 1) {
      return { ml: 0 };
    }
    if (isLocalTab) {
      return {
        ml: 6,
      };
    }
  }, [ isLocalTab, tabs.length ]);

  const tabsListProps = React.useMemo(() => {
    if (tabs.length === 1) {
      return { display: 'none' };
    }
    return {
      marginBottom: 0,
      pt: 6,
      pb: 3,
      marginTop: -6,
    };
  }, [ tabs.length ]);

  if (!txQuery.isPending && !txQuery.isPlaceholderData && !txQuery.isError && !txQuery.data.status) {
    return txQuery.socketStatus ? <TxSocketAlert status={ txQuery.socketStatus }/> : <TxPendingAlert/>;
  }

  const rightSlot = (() => {
    if (isLocalTab) {
      if (!typeFilter.length && !localQuery.data?.items.length) {
        return null;
      }
      return (
        <>
          <TokenTransferFilter
            defaultTypeFilters={ typeFilter }
            onTypeFilterChange={ handleTypeFilterChange }
            appliedFiltersNum={ typeFilter.length }
            isLoading={ txQuery.isPlaceholderData || localQuery.isPlaceholderData }
            chainConfig={ multichainContext?.chain?.app_config }
          />
          <Pagination ml="auto" { ...localQuery.pagination }/>
        </>
      );
    }

    return (
      <Pagination ml="auto" { ...crossChainQuery.pagination }/>
    );
  })();

  return (
    <RoutedTabs
      tabs={ tabs }
      isLoading={ isTabsLoading }
      variant="secondary"
      size="sm"
      listProps={ tabsListProps }
      rightSlot={ rightSlot }
      rightSlotProps={ rightSlotProps }
      stickyEnabled={ !isMobile }
    />
  );
};

export default TxTokenTransfer;
