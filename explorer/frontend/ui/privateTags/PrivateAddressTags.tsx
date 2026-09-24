import { Box } from '@chakra-ui/react';
import React, { useCallback, useState } from 'react';

import type { AddressTag } from 'types/api/account';

import { PAGE_TYPE_DICT } from 'lib/mixpanel/getPageType';
import { PRIVATE_TAG_ADDRESS } from 'stubs/account';
import { Button } from 'toolkit/chakra/button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AccountPageDescription from 'ui/shared/AccountPageDescription';
import ActionBar, { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import Pagination from 'ui/shared/pagination/Pagination';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';

import AddressModal from './AddressModal/AddressModal';
import AddressTagListItem from './AddressTagTable/AddressTagListItem';
import AddressTagTable from './AddressTagTable/AddressTagTable';
import DeletePrivateTagModal from './DeletePrivateTagModal';

const PrivateAddressTags = () => {
  const { data: addressTagsData, isError, isPlaceholderData, refetch, pagination } = useQueryWithPages({
    resourceName: 'general:private_tags_address',
    options: {
      refetchOnMount: false,
      placeholderData: { items: Array(5).fill(PRIVATE_TAG_ADDRESS), next_page_params: null },
    },
  });

  const addressModalProps = useDisclosure();
  const deleteModalProps = useDisclosure();

  const [ addressModalData, setAddressModalData ] = useState<AddressTag>();
  const [ deleteModalData, setDeleteModalData ] = useState<AddressTag>();

  const onEditClick = useCallback((data: AddressTag) => {
    setAddressModalData(data);
    addressModalProps.onOpen();
  }, [ addressModalProps ]);

  const onAddOrEditSuccess = useCallback(async() => {
    await refetch();
  }, [ refetch ]);

  const onAddressModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setAddressModalData(undefined);
    addressModalProps.onOpenChange({ open });
  }, [ addressModalProps ]);

  const onDeleteClick = useCallback((data: AddressTag) => {
    setDeleteModalData(data);
    deleteModalProps.onOpen();
  }, [ deleteModalProps ]);

  const onDeleteModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setDeleteModalData(undefined);
    deleteModalProps.onOpenChange({ open });
  }, [ deleteModalProps ]);

  const actionBar = pagination.isVisible ? (
    <ActionBar mt={ -6 }>
      <Pagination ml="auto" { ...pagination }/>
    </ActionBar>
  ) : null;

  return (
    <>
      <AccountPageDescription>
        Use private address tags to track any addresses of interest.
        Private tags are saved in your account and are only visible when you are logged in.
      </AccountPageDescription>
      <DataListDisplay
        isError={ isError }
        itemsNum={ addressTagsData?.items.length }
        emptyText=""
        actionBar={ actionBar }
      >
        <Box display={{ base: 'block', lg: 'none' }}>
          { addressTagsData?.items.map((item: AddressTag, index: number) => (
            <AddressTagListItem
              item={ item }
              key={ item.id + (isPlaceholderData ? String(index) : '') }
              onDeleteClick={ onDeleteClick }
              onEditClick={ onEditClick }
              isLoading={ isPlaceholderData }
            />
          )) }
        </Box>
        <Box display={{ base: 'none', lg: 'block' }}>
          <AddressTagTable
            isLoading={ isPlaceholderData }
            data={ addressTagsData?.items }
            onDeleteClick={ onDeleteClick }
            onEditClick={ onEditClick }
            top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          />
        </Box>
      </DataListDisplay>
      <Skeleton mt={ 8 } loading={ isPlaceholderData } display="inline-block">
        <Button
          onClick={ addressModalProps.onOpen }
        >
          Add address tag
        </Button>
      </Skeleton>
      <AddressModal
        { ...addressModalProps }
        data={ addressModalData }
        pageType={ PAGE_TYPE_DICT['/account/tag-address'] }
        onOpenChange={ onAddressModalOpenChange }
        onSuccess={ onAddOrEditSuccess }
      />
      { deleteModalData && (
        <DeletePrivateTagModal
          { ...deleteModalProps }
          onOpenChange={ onDeleteModalOpenChange }
          data={ deleteModalData }
          type="address"
        />
      ) }
    </>
  );
};

export default PrivateAddressTags;
