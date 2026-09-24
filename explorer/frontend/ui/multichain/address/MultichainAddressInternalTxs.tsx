import { Box } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import AddressCsvExportLink from 'ui/address/AddressCsvExportLink';
import AddressTxsFilter from 'ui/address/AddressTxsFilter';
import useAddressInternalTxsQuery from 'ui/address/useAddressInternalTxsQuery';
import InternalTxsList from 'ui/internalTxs/InternalTxsList';
import InternalTxsTable from 'ui/internalTxs/InternalTxsTable';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';

import getAvailableChainIds from './getAvailableChainIds';

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressInternalTxs = ({ addressData, isLoading }: Props) => {
  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);

  const { hash, query, filterValue, onFilterChange } = useAddressInternalTxsQuery({
    enabled: !isLoading && chainIds.length > 0,
    isMultichain: true,
    chainIds,
  });
  const { data, isPlaceholderData, isError, pagination, chainValue, onChainValueChange } = query;

  const chainData = React.useMemo(() => {
    const config = multichainConfig();
    return config?.chains.find(({ id }) => id === chainValue?.[0]);
  }, [ chainValue ]);

  if (chainIds.length === 0) {
    return <p>There are no internal transactions.</p>;
  }

  const content = data?.items ? (
    <MultichainProvider chainId={ chainValue?.[0] }>
      <Box hideFrom="lg">
        <InternalTxsList data={ data.items } currentAddress={ hash } isLoading={ isPlaceholderData }/>
      </Box>
      <Box hideBelow="lg">
        <InternalTxsTable data={ data.items } currentAddress={ hash } isLoading={ isPlaceholderData }/>
      </Box>
    </MultichainProvider>
  ) : null ;

  const actionBar = (
    <ActionBar mt={ -6 } justifyContent="left">
      <AddressTxsFilter
        initialValue={ filterValue }
        onFilterChange={ onFilterChange }
        hasActiveFilter={ Boolean(filterValue) }
        isLoading={ pagination.isLoading }
      />
      <ChainSelect
        loading={ pagination.isLoading }
        value={ chainValue }
        onValueChange={ onChainValueChange }
        chainIds={ chainIds }
        ml={ 2 }
      />
      <AddressCsvExportLink
        address={ hash }
        isLoading={ pagination.isLoading }
        params={{ type: 'internal-transactions', filterType: 'address', filterValue }}
        ml={{ base: 2, lg: 'auto' }}
        chainData={ chainData }
      />
      <Pagination ml={{ base: 'auto', lg: 8 }} { ...pagination }/>
    </ActionBar>
  );

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items.length }
      hasActiveFilters={ Boolean(filterValue) }
      emptyStateProps={{
        term: 'transaction',
      }}
      emptyText="There are no internal transactions."
      showActionBarIfEmpty
      showActionBarIfError
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(MultichainAddressInternalTxs);
