import React from 'react';

import type { ClusterByNameResponse } from 'types/api/clusters';

import { isEvmAddress } from 'lib/address/isEvmAddress';
import { currencyUnits } from 'lib/units';
import { Skeleton } from 'toolkit/chakra/skeleton';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ClustersEntity from 'ui/shared/entities/clusters/ClustersEntity';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  clusterData?: ClusterByNameResponse['result']['data'];
  clusterName: string;
  isLoading: boolean;
}

const ClusterDetails = ({ clusterData, clusterName, isLoading }: Props) => {
  if (!clusterData && !isLoading) {
    throw new Error('Cluster not found', { cause: { status: 404 } });
  }

  const ownerIsEvm = clusterData?.owner ? isEvmAddress(clusterData.owner) : false;
  const addressType = ownerIsEvm ? 'EVM' : 'NON-EVM';

  return (
    <DetailedInfo.Container>
      <DetailedInfo.ItemLabel
        hint="The unique cluster name"
        isLoading={ isLoading }
      >
        Cluster name
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <ClustersEntity
          clusterName={ clusterName }
          isLoading={ isLoading }
          noLink
          fontWeight={ 500 }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The address attached to this cluster name"
        isLoading={ isLoading }
      >
        Owner address
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <AddressEntity
          address={{ hash: clusterData?.owner || '' }}
          isLoading={ isLoading }
          fontWeight={ 500 }
          noLink={ !ownerIsEvm }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="The network type of the address attached to this cluster name"
        isLoading={ isLoading }
      >
        Type
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isLoading }>
          { addressType }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint={ `The amount of ${ currencyUnits.ether } backing this cluster name` }
        isLoading={ isLoading }
      >
        Backing
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <NativeCoinValue
          amount={ clusterData?.backingWei || '0' }
          loading={ isLoading }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="When this cluster name was created"
        isLoading={ isLoading }
      >
        Created
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        { clusterData?.createdAt ? (
          <DetailedInfoTimestamp
            timestamp={ clusterData.createdAt }
            isLoading={ isLoading }
          />
        ) : (
          <Skeleton loading={ isLoading }>N/A</Skeleton>
        ) }
      </DetailedInfo.ItemValue>
    </DetailedInfo.Container>
  );
};

export default ClusterDetails;
