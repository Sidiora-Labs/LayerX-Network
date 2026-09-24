import { Box } from '@chakra-ui/react';
import { BigNumber } from 'bignumber.js';
import React from 'react';

import { MultichainProvider } from 'lib/contexts/multichain';
import getItemIndex from 'lib/getItemIndex';
import { TOP_ADDRESS } from 'stubs/address';
import { generateListStub } from 'stubs/utils';
import AddressesListItem from 'ui/addresses/AddressesListItem';
import AddressesTable from 'ui/addresses/AddressesTable';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

const MultichainAccounts = () => {
  const { isError, isPlaceholderData, data, pagination, chainValue, onChainValueChange } = useQueryWithPages({
    resourceName: 'general:addresses',
    options: {
      placeholderData: generateListStub<'general:addresses'>(TOP_ADDRESS, 50, {
        next_page_params: {
          fetched_coin_balance: '42',
          hash: '0x99f0ec06548b086e46cb0019c78d0b9b9f36cd53',
          items_count: 50,
        },
        total_supply: '0',
      }),
    },
    isMultichain: true,
  });

  const pageStartIndex = getItemIndex(0, pagination.page);
  const totalSupply = React.useMemo(() => {
    return BigNumber(data?.total_supply || '0');
  }, [ data?.total_supply ]);

  const content = data?.items ? (
    <MultichainProvider chainId={ chainValue?.[0] }>
      <Box hideBelow="lg">
        <AddressesTable
          top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          items={ data.items }
          totalSupply={ totalSupply }
          pageStartIndex={ pageStartIndex }
          isLoading={ isPlaceholderData }
        />
      </Box>
      <Box hideFrom="lg">
        { data.items.map((item, index) => (
          <AddressesListItem
            key={ item.hash + (isPlaceholderData ? index : '') }
            item={ item }
            index={ pageStartIndex + index }
            totalSupply={ totalSupply }
            isLoading={ isPlaceholderData }
          />
        )) }
      </Box>
    </MultichainProvider>
  ) : null;

  const actionBar = (
    <ActionBar mt={ -6 }>
      <ChainSelect
        value={ chainValue }
        onValueChange={ onChainValueChange }
      />
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  );

  return (
    <>
      <PageTitle
        title="Top accounts"
        withTextAd
      />
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items?.length }
        emptyText="There are no accounts."
        actionBar={ actionBar }
        showActionBarIfError
        showActionBarIfEmpty
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default React.memo(MultichainAccounts);
