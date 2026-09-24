import React from 'react';

import type { ClustersDirectoryObject } from 'types/api/clusters';

import { isEvmAddress } from 'lib/address/isEvmAddress';
import dayjs from 'lib/date/dayjs';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ClustersEntity from 'ui/shared/entities/clusters/ClustersEntity';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import Time from 'ui/shared/time/Time';

interface Props {
  item: ClustersDirectoryObject;
  isLoading?: boolean;
  isClusterDetailsLoading?: boolean;
}

const ClustersDirectoryListItem = ({ item, isLoading, isClusterDetailsLoading }: Props) => {
  return (
    <ListItemMobileGrid.Container>
      <ListItemMobileGrid.Label isLoading={ isLoading }>
        Cluster name
      </ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <ClustersEntity clusterName={ item.name } isLoading={ isLoading } fontWeight={ 500 }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>
        Address
      </ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        { item.owner && (
          <AddressEntity
            address={{ hash: item.owner }}
            isLoading={ isLoading }
            fontWeight={ 500 }
            noLink={ !isEvmAddress(item.owner) }
          />
        ) }
        { !item.owner && <Skeleton loading={ isLoading }>—</Skeleton> }
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>
        Joined
      </ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading }>
          <Time timestamp={ item.createdAt } display="block"/>
          <div> { dayjs(item.createdAt).fromNow() }</div>
        </Skeleton>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>
        Active chains
      </ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading || isClusterDetailsLoading }>
          { (item.chainIds?.length || 1) } { (item.chainIds?.length || 1) === 1 ? 'chain' : 'chains' }
        </Skeleton>
      </ListItemMobileGrid.Value>
    </ListItemMobileGrid.Container>
  );
};

export default React.memo(ClustersDirectoryListItem);
