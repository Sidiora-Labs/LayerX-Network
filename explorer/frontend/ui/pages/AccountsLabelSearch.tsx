import { Box, chakra, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { EntityTag as TEntityTag, EntityTagType } from 'ui/shared/EntityTags/types';

import getQueryParamString from 'lib/router/getQueryParamString';
import { TOP_ADDRESS } from 'stubs/address';
import { generateListStub } from 'stubs/utils';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressesLabelSearchListItem from 'ui/addressesLabelSearch/AddressesLabelSearchListItem';
import AddressesLabelSearchTable from 'ui/addressesLabelSearch/AddressesLabelSearchTable';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import DataListDisplay from 'ui/shared/DataListDisplay';
import EntityTag from 'ui/shared/EntityTags/EntityTag';
import PageTitle from 'ui/shared/Page/PageTitle';
import useQueryWithPages from 'ui/shared/pagination/useQueryWithPages';
import StickyPaginationWithText from 'ui/shared/StickyPaginationWithText';

const AccountsLabelSearch = () => {

  const router = useRouter();
  const slug = getQueryParamString(router.query.slug);
  const tagType = getQueryParamString(router.query.tagType);
  const tagName = getQueryParamString(router.query.tagName);

  const { isError, isPlaceholderData, data, pagination } = useQueryWithPages({
    resourceName: 'general:addresses_metadata_search',
    filters: {
      slug,
      tag_type: tagType,
    },
    options: {
      placeholderData: generateListStub<'general:addresses_metadata_search'>(
        TOP_ADDRESS,
        50,
        {
          next_page_params: null,
        },
      ),
    },
  });

  const content = data?.items ? (
    <>
      <Box hideBelow="lg">
        <AddressesLabelSearchTable
          top={ pagination.isVisible ? ACTION_BAR_HEIGHT_DESKTOP : 0 }
          items={ data.items }
          isLoading={ isPlaceholderData }
        />
      </Box>
      <Box hideFrom="lg">
        { data.items.map((item, index) => {
          return (
            <AddressesLabelSearchListItem
              key={ item.hash + (isPlaceholderData ? index : '') }
              item={ item }
              isLoading={ isPlaceholderData }
            />
          );
        }) }
      </Box>
    </>
  ) : null;

  const text = (() => {
    if (isError) {
      return null;
    }

    const num = data?.items.length || 0;

    const tagData: TEntityTag = {
      tagType: tagType as EntityTagType,
      slug,
      name: tagName || slug,
      ordinal: 0,
    };

    return (
      <Flex alignItems="center" columnGap={ 2 } flexWrap="wrap" rowGap={ 1 }>
        <Skeleton loading={ isPlaceholderData } display="inline-block">
          Found{ ' ' }
          <chakra.span fontWeight={ 700 }>
            { num }{ data?.next_page_params || pagination.page > 1 ? '+' : '' }
          </chakra.span>{ ' ' }
          matching result{ num > 1 ? 's' : '' } for
        </Skeleton>
        <EntityTag data={ tagData } isLoading={ isPlaceholderData } noLink/>
      </Flex>
    );
  })();

  const actionBar = <StickyPaginationWithText text={ text } pagination={ pagination }/>;

  return (
    <>
      <PageTitle title="Search result" withTextAd/>
      <DataListDisplay
        isError={ isError }
        itemsNum={ data?.items.length }
        emptyText={ text }
        actionBar={ actionBar }
      >
        { content }
      </DataListDisplay>
    </>
  );
};

export default AccountsLabelSearch;
