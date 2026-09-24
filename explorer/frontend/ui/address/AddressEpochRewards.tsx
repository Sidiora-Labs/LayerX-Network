import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import useIsMounted from 'lib/hooks/useIsMounted';
import getQueryParamString from 'lib/router/getQueryParamString';
import { EPOCH_REWARD_ITEM } from 'stubs/address';
import { generateListStub } from 'stubs/utils';
import AddressEpochRewardsTable from 'ui/address/epochRewards/AddressEpochRewardsTable';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import AddressCsvExportLink from './AddressCsvExportLink';
import AddressEpochRewardsListItem from './epochRewards/AddressEpochRewardsListItem';

type Props = {
  shouldRender?: boolean;
  isQueryEnabled?: boolean;
};

const AddressEpochRewards = ({ shouldRender = true, isQueryEnabled = true }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();

  const hash = getQueryParamString(router.query.hash);

  const rewardsQuery = useQueryWithPages({
    resourceName: 'general:address_epoch_rewards',
    pathParams: {
      hash,
    },
    options: {
      enabled: isQueryEnabled && Boolean(hash),
      placeholderData: generateListStub<'general:address_epoch_rewards'>(EPOCH_REWARD_ITEM, 50, { next_page_params: {
        amount: '1',
        items_count: 50,
        type: 'voter',
        associated_account_address_hash: '1',
        epoch_number: 10355938,
      } }),
    },
  });

  if (!isMounted || !shouldRender) {
    return null;
  }

  const content = rewardsQuery.data?.items ? (
    <>
      <Box hideBelow="lg">
        <AddressEpochRewardsTable
          items={ rewardsQuery.data.items }
          top={ rewardsQuery.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          isLoading={ rewardsQuery.isPlaceholderData }
        />
      </Box>
      <Box hideFrom="lg">
        { rewardsQuery.data.items.map((item, index) => (
          <AddressEpochRewardsListItem
            key={ item.epoch_number + item.type + item.account.hash + item.associated_account.hash + (rewardsQuery.isPlaceholderData ? String(index) : '') }
            item={ item }
            isLoading={ rewardsQuery.isPlaceholderData }
          />
        )) }
      </Box>
    </>
  ) : null;

  const actionBar = (
    <ActionBar mt={ -6 }>
      <AddressCsvExportLink
        address={ hash }
        isLoading={ rewardsQuery.pagination.isLoading }
        params={{ type: 'epoch-rewards' }}
        ml={{ lg: 'auto' }}
      />
      { rewardsQuery.pagination.isVisible && (
        <Pagination
          ml={{ base: 0, lg: 8 }}
          { ...rewardsQuery.pagination }
        />
      ) }
    </ActionBar>
  );

  return (
    <DataListDisplay
      isError={ rewardsQuery.isError }
      itemsNum={ rewardsQuery.data?.items?.length }
      emptyText="There are no epoch rewards for this address."
      actionBar={ actionBar }
    >
      { content }
    </DataListDisplay>
  );
};

export default AddressEpochRewards;
