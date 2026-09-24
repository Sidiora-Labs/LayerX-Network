import React from 'react';

import type { AddressImplementation } from 'types/api/addressParams';
import type { SmartContractProxyType } from 'types/api/contract';

import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';

interface Props {
  data: Array<AddressImplementation>;
  isLoading?: boolean;
  proxyType?: SmartContractProxyType;
}

const AddressImplementations = ({ data, isLoading, proxyType }: Props) => {
  const hasManyItems = data.length > 1;
  const [ hasScroll, setHasScroll ] = React.useState(false);

  const text = proxyType === 'eip7702' ? 'Delegated to' : `Implementation${ hasManyItems ? 's' : '' }`;
  const hint = proxyType === 'eip7702' ?
    'Account\'s executable code address' :
    `Implementation${ hasManyItems ? 's' : '' } address${ hasManyItems ? 'es' : '' } of the proxy contract`;

  return (
    <>
      <DetailedInfo.ItemLabel
        hint={ hint }
        isLoading={ isLoading }
        hasScroll={ hasScroll }
      >
        { text }
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValueWithScroll
        gradientHeight={ 48 }
        onScrollVisibilityChange={ setHasScroll }
        rowGap={ 2 }
        maxH="200px"
      >
        { data.map((item) => (
          <AddressEntity
            key={ item.address_hash }
            address={{
              hash: item.address_hash,
              filecoin: { robust: item.filecoin_robust_address },
              name: item.name,
              is_contract: true,
            }}
            isLoading={ isLoading }
            noIcon
          />
        )) }
      </DetailedInfo.ItemValueWithScroll>
    </>
  );
};

export default React.memo(AddressImplementations);
