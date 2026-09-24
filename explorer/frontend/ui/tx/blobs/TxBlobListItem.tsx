import React from 'react';

import type { TxBlob } from 'types/api/blobs';

import { Skeleton } from 'toolkit/chakra/skeleton';
import BlobDataType from 'ui/shared/blob/BlobDataType';
import BlobEntity from 'ui/shared/entities/blob/BlobEntity';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';

interface Props {
  data: TxBlob;
  isLoading?: boolean;
}

const TxBlobListItem = ({ data, isLoading }: Props) => {
  const size = data.blob_data ? data.blob_data.replace('0x', '').length / 2 : '-';

  return (
    <ListItemMobileGrid.Container>
      <ListItemMobileGrid.Label isLoading={ isLoading }>Blob hash</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <BlobEntity hash={ data.hash } isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Data type</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        { data.blob_data ? <BlobDataType isLoading={ isLoading } data={ data.blob_data }/> : '-' }
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Size, bytes</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading }>
          { size.toLocaleString() }
        </Skeleton>
      </ListItemMobileGrid.Value>
    </ListItemMobileGrid.Container>
  );
};

export default React.memo(TxBlobListItem);
