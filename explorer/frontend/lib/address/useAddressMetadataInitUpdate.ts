import React from 'react';

import type { AddressCounters } from 'types/api/address';

import config from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';

const feature = config.features.addressMetadata;

interface Params {
  address: string | undefined;
  counters: AddressCounters | undefined;
  isEnabled: boolean;
}

const TXS_THRESHOLD = 500;

export default function useAddressMetadataInitUpdate({ address, counters, isEnabled }: Params) {

  const apiFetch = useApiFetch();

  React.useEffect(() => {
    if (
      feature.isEnabled &&
        feature.isAddressTagsUpdateEnabled &&
        address &&
        isEnabled &&
        counters?.transactions_count && Number(counters.transactions_count) > TXS_THRESHOLD
    ) {
      apiFetch('metadata:address_submit', {
        fetchParams: {
          method: 'POST',
          body: {
            addresses: [ address ],
          },
        },
      });
    }
  }, [ address, apiFetch, counters?.transactions_count, isEnabled ]);
}
