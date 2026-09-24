import type { UseQueryResult } from '@tanstack/react-query';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { AddressCounters } from 'types/api/address';

import { route } from 'nextjs/routes';

import type { ResourceError } from 'lib/api/resources';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';

interface Props {
  prop: keyof AddressCounters;
  query: UseQueryResult<AddressCounters, ResourceError<unknown>>;
  address: string;
  isAddressQueryLoading: boolean;
  isDegradedData: boolean;
}

const PROP_TO_TAB = {
  transactions_count: 'txs',
  token_transfers_count: 'token_transfers',
  validations_count: 'blocks_validated',
};

const AddressCounterItem = ({ prop, query, address, isAddressQueryLoading, isDegradedData }: Props) => {
  const handleClick = React.useCallback(() => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, []);

  if (query.isPlaceholderData || isAddressQueryLoading) {
    return <Skeleton loading h={ 5 } w="80px" borderRadius="full"/>;
  }

  const data = query.data?.[prop];

  if (query.isError || data === null || data === undefined) {
    return <span>0</span>;
  }

  switch (prop) {
    case 'gas_usage_count':
      return <span>{ BigNumber(data).toFormat() }</span>;
    case 'transactions_count':
    case 'token_transfers_count':
    case 'validations_count': {
      if (data === '0') {
        return <span>0</span>;
      }

      if (isDegradedData) {
        return <span>{ Number(data).toLocaleString() }</span>;
      }

      return (
        <Link
          href={ route({ pathname: '/address/[hash]', query: { hash: address, tab: PROP_TO_TAB[prop] } }) }
          scroll={ false }
          onClick={ handleClick }
        >
          { Number(data).toLocaleString() }
        </Link>
      );
    }
  }
};

export default React.memo(AddressCounterItem);
