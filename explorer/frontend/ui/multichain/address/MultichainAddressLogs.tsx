import { useRouter } from 'next/router';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import getQueryParamString from 'lib/router/getQueryParamString';
import { LOG } from 'stubs/log';
import { generateListStub } from 'stubs/utils';
import AddressCsvExportLink from 'ui/address/AddressCsvExportLink';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import ActionBar from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import LogItem from 'ui/shared/logs/LogItem';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import getAvailableChainIds from './getAvailableChainIds';

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressLogs = ({ addressData, isLoading }: Props) => {
  const router = useRouter();
  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);
  const isMobile = useIsMobile();

  const hash = getQueryParamString(router.query.hash);
  const { data, isPlaceholderData, isError, pagination, chainValue, onChainValueChange } = useQueryWithPages({
    resourceName: 'general:address_logs',
    pathParams: { hash },
    options: {
      placeholderData: generateListStub<'general:address_logs'>(LOG, 3, { next_page_params: {
        block_number: 9005750,
        index: 42,
        items_count: 50,
        transaction_index: 23,
      } }),
      enabled: !isLoading,
    },
    isMultichain: true,
    chainIds,
  });

  const chainData = React.useMemo(() => {
    const config = multichainConfig();
    return config?.chains.find(({ id }) => id === chainValue?.[0]);
  }, [ chainValue ]);

  const actionBar = (
    <ActionBar mt={ -6 }>
      <ChainSelect
        value={ chainValue }
        onValueChange={ onChainValueChange }
        chainIds={ chainIds }
        loading={ isLoading }
        mode={ isMobile ? 'compact' : 'default' }
      />
      { (data?.items.length ?? 0) > 0 && (
        <AddressCsvExportLink
          address={ hash }
          isLoading={ pagination.isLoading }
          params={{ type: 'logs' }}
          ml={{ base: 2, lg: 'auto' }}
          chainData={ chainData }
        />
      ) }
      <Pagination ml={{ base: 'auto', lg: 8 }} { ...pagination }/>
    </ActionBar>
  );

  const content = data?.items ? data.items.map((item, index) => (
    <LogItem
      key={ index }
      { ...item }
      type="address"
      isLoading={ isPlaceholderData }
      chainData={ chainData }
    />
  )) : null;

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items?.length }
      emptyText="There are no logs for this address."
      showActionBarIfEmpty
      showActionBarIfError
      actionBar={ actionBar }
    >
      <MultichainProvider chainId={ chainValue?.[0] }>
        { content }
      </MultichainProvider>
    </DataListDisplay>
  );
};

export default MultichainAddressLogs;
