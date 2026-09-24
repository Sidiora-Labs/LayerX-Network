import { Box } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type * as bens from '@blockscout/bens-types';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { ENS_DOMAIN_EVENT } from 'stubs/ENS';
import DataListDisplay from 'ui/shared/DataListDisplay';

import NameDomainHistoryListItem from './history/NameDomainHistoryListItem';
import NameDomainHistoryTable from './history/NameDomainHistoryTable';
import { getNextSortValue, type Sort, type SortField } from './history/utils';

const feature = config.features.nameServices;
const availableProtocols = feature.isEnabled && feature.ens.isEnabled ? feature.ens.protocols : [];

interface Props {
  domain: bens.DetailedDomain | undefined;
}

const NameDomainHistory = ({ domain }: Props) => {
  const router = useRouter();
  const domainName = getQueryParamString(router.query.name);
  const protocolId = getQueryParamString(router.query.protocol_id) || availableProtocols[0];

  const [ sort, setSort ] = React.useState<Sort>('default');

  const { isPlaceholderData, isError, data } = useApiQuery('bens:domain_events', {
    pathParams: { name: domainName },
    queryParams: {
      protocol_id: protocolId,
    },
    queryOptions: {
      placeholderData: { items: Array(4).fill(ENS_DOMAIN_EVENT) },
    },
  });

  const handleSortToggle = React.useCallback((field: SortField) => {
    if (isPlaceholderData) {
      return;
    }

    if (field) {
      setSort(getNextSortValue(field));
    }
  }, [ isPlaceholderData ]);

  const content = (
    <>
      <Box hideFrom="lg">
        { data?.items.map((item, index) => (
          <NameDomainHistoryListItem
            key={ index }
            event={ item }
            domain={ domain }
            isLoading={ isPlaceholderData }
          />
        )) }
      </Box>
      <Box hideBelow="lg">
        <NameDomainHistoryTable
          history={ data }
          domain={ domain }
          isLoading={ isPlaceholderData }
          sort={ sort }
          onSortToggle={ handleSortToggle }
        />
      </Box>
    </>
  );

  return (
    <DataListDisplay
      isError={ isError }
      itemsNum={ data?.items.length }
      emptyText="There are no events for this domain."
    >
      { content }
    </DataListDisplay>
  );
};

export default React.memo(NameDomainHistory);
