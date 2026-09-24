import { Box } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React, { useCallback, useState } from 'react';

import type { WatchlistAddress, WatchlistResponse } from 'types/api/account';

import { resourceKey } from 'lib/api/resources';
import { getResourceKey } from 'lib/api/useApiQuery';
import { WATCH_LIST_ITEM_WITH_TOKEN_INFO } from 'stubs/account';
import { Button } from 'toolkit/chakra/button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AccountPageDescription from 'ui/shared/AccountPageDescription';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';
import useRedirectForInvalidAuthToken from 'ui/snippets/auth/useRedirectForInvalidAuthToken';
import AddressModal from 'ui/watchlist/AddressModal/AddressModal';
import DeleteAddressModal from 'ui/watchlist/DeleteAddressModal';
import WatchlistEmailAlert from 'ui/watchlist/WatchlistEmailAlert';
import WatchListItem from 'ui/watchlist/WatchlistTable/WatchListItem';
import WatchlistTable from 'ui/watchlist/WatchlistTable/WatchlistTable';

const WatchList: React.FC = () => {

  const { data, isPlaceholderData, isError, pagination } = useQueryWithPages({
    resourceName: 'general:watchlist',
    options: {
      placeholderData: { items: Array(5).fill(WATCH_LIST_ITEM_WITH_TOKEN_INFO), next_page_params: null },
    },
  });
  const queryClient = useQueryClient();
  const profileQuery = useProfileQuery();

  const addressModalProps = useDisclosure();
  const deleteModalProps = useDisclosure();
  useRedirectForInvalidAuthToken();

  const [ addressModalData, setAddressModalData ] = useState<WatchlistAddress>();
  const [ deleteModalData, setDeleteModalData ] = useState<WatchlistAddress>();

  const hasEmail = Boolean(profileQuery.data?.email);

  const onEditClick = useCallback((data: WatchlistAddress) => {
    setAddressModalData(data);
    addressModalProps.onOpen();
  }, [ addressModalProps ]);

  const onAddressModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setAddressModalData(undefined);
    addressModalProps.onOpenChange({ open });
  }, [ addressModalProps ]);

  const onAddOrEditSuccess = useCallback(async() => {
    await queryClient.refetchQueries({ queryKey: [ resourceKey('general:watchlist') ] });
    setAddressModalData(undefined);
    addressModalProps.onClose();
  }, [ addressModalProps, queryClient ]);

  const onDeleteClick = useCallback((data: WatchlistAddress) => {
    setDeleteModalData(data);
    deleteModalProps.onOpen();
  }, [ deleteModalProps ]);

  const onDeleteModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setDeleteModalData(undefined);
    deleteModalProps.onOpenChange({ open });
  }, [ deleteModalProps ]);

  const onDeleteSuccess = useCallback(async() => {
    queryClient.setQueryData(getResourceKey('general:watchlist'), (prevData: WatchlistResponse | undefined) => {
      const newItems = prevData?.items.filter((item: WatchlistAddress) => item.id !== deleteModalData?.id);
      return { ...prevData, items: newItems };
    },
    );
  }, [ deleteModalData?.id, queryClient ]);

  const content = (() => {
    const actionBar = pagination.isVisible ? (
      <ActionBar mt={ -6 }>
        <Pagination ml="auto" { ...pagination }/>
      </ActionBar>
    ) : null;

    return (
      <>
        { !hasEmail && <WatchlistEmailAlert/> }
        <AccountPageDescription>
          An email notification can be sent to you when an address on your watch list sends or receives any transactions.
        </AccountPageDescription>
        <DataListDisplay
          isError={ isError }
          itemsNum={ data?.items.length }
          emptyText=""
          actionBar={ actionBar }
        >
          <Box display={{ base: 'block', lg: 'none' }}>
            { data?.items.map((item, index) => (
              <WatchListItem
                key={ item.address_hash + (isPlaceholderData ? index : '') }
                item={ item }
                isLoading={ isPlaceholderData }
                onDeleteClick={ onDeleteClick }
                onEditClick={ onEditClick }
                hasEmail={ hasEmail }
              />
            )) }
          </Box>
          <Box display={{ base: 'none', lg: 'block' }}>
            <WatchlistTable
              data={ data?.items }
              isLoading={ isPlaceholderData }
              onDeleteClick={ onDeleteClick }
              onEditClick={ onEditClick }
              top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
              hasEmail={ hasEmail }
            />
          </Box>
        </DataListDisplay>
        <Skeleton mt={ 8 } loading={ isPlaceholderData } display="inline-block">
          <Button
            onClick={ addressModalProps.onOpen }
          >
            Add address
          </Button>
        </Skeleton>
        <AddressModal
          { ...addressModalProps }
          onOpenChange={ onAddressModalOpenChange }
          onSuccess={ onAddOrEditSuccess }
          data={ addressModalData }
          isAdd={ !addressModalData }
          hasEmail={ hasEmail }
        />
        { deleteModalData && (
          <DeleteAddressModal
            { ...deleteModalProps }
            onOpenChange={ onDeleteModalOpenChange }
            onSuccess={ onDeleteSuccess }
            data={ deleteModalData }
          />
        ) }
      </>
    );
  })();

  return (
    <>
      <PageTitle title="Watch list"/>
      { content }
    </>
  );
};

export default WatchList;
