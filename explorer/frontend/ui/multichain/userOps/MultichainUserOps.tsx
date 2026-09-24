import { Box } from '@chakra-ui/react';
import React from 'react';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import { USER_OPS_ITEM } from 'stubs/userOps';
import { generateListStub } from 'stubs/utils';
import ChainSelect from 'ui/multichain/components/ChainSelect';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import UserOpsListItem from 'ui/userOps/UserOpsListItem';
import UserOpsTable from 'ui/userOps/UserOpsTable';

const MultichainUserOps = () => {

  const chains = React.useMemo(() => (multichainConfig()?.chains || []).filter(chain => chain.app_config.features.userOps.isEnabled), []);
  const chainIds = React.useMemo(() => chains.map(chain => chain.id).filter(Boolean), [ chains ]);

  const query = useQueryWithPages({
    resourceName: 'general:user_ops',
    options: {
      placeholderData: generateListStub<'general:user_ops'>(USER_OPS_ITEM, 50, { next_page_params: {
        page_token: '10355938',
        page_size: 50,
      } }),
    },
    isMultichain: true,
    chainIds,
  });

  const chainConfig = chains.find(chain => chain.id === query.chainValue?.[0]);

  const content = query.data?.items ? (
    <MultichainProvider chainId={ query.chainValue?.[0] }>
      <Box hideBelow="lg">
        <UserOpsTable
          items={ query.data.items }
          top={ query.pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          isLoading={ query.isPlaceholderData }
          showTx
          showSender
        />
      </Box>
      <Box hideFrom="lg">
        { query.data.items.map((item, index) => (
          <UserOpsListItem
            key={ item.hash + (query.isPlaceholderData ? String(index) : '') }
            item={ item }
            isLoading={ query.isPlaceholderData }
            showTx
            showSender
            chainData={ chainConfig }
          />
        )) }
      </Box>
    </MultichainProvider>
  ) : null;

  const actionBar = (
    <ActionBar mt={ -6 }>
      <ChainSelect
        value={ query.chainValue }
        onValueChange={ query.onChainValueChange }
        chainIds={ chainIds }
      />
      <Pagination ml="auto" { ...query.pagination }/>
    </ActionBar>
  );

  return (
    <>
      <PageTitle
        title="User operations"
        withTextAd
      />
      <DataListDisplay
        isError={ query.isError }
        itemsNum={ query.data?.items?.length }
        emptyText="There are no user operations."
        actionBar={ actionBar }
        showActionBarIfError
        showActionBarIfEmpty
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default React.memo(MultichainUserOps);
