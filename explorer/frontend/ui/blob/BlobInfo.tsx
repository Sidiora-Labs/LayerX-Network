import { GridItem } from '@chakra-ui/react';
import React from 'react';

import type { Blob } from 'types/api/blobs';

import { Alert } from 'toolkit/chakra/alert';
import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoSponsoredItem from 'ui/shared/DetailedInfo/DetailedInfoSponsoredItem';
import TxEntity from 'ui/shared/entities/tx/TxEntity';

import BlobData from './BlobData';

interface Props {
  data: Blob;
  isLoading?: boolean;
}

const BlobInfo = ({ data, isLoading }: Props) => {
  return (
    <DetailedInfo.Container
      templateColumns={{ base: 'minmax(0, 1fr)', lg: '216px minmax(728px, auto)' }}
    >
      { !data.blob_data && (
        <GridItem colSpan={{ base: undefined, lg: 2 }} mb={ 3 }>
          <Skeleton loading={ isLoading }>
            <Alert status="warning">This blob is not yet indexed</Alert>
          </Skeleton>
        </GridItem>
      ) }

      { data.kzg_proof && (
        <>
          <DetailedInfo.ItemLabel
            hint="Zero knowledge proof. Allows for quick verification of commitment"
            isLoading={ isLoading }
          >
            Proof
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isLoading } overflow="hidden" whiteSpace="pre-wrap" wordBreak="break-all">
              { data.kzg_proof }
              <CopyToClipboard text={ data.kzg_proof } isLoading={ isLoading }/>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.kzg_commitment && (
        <>
          <DetailedInfo.ItemLabel
            hint="Commitment to the data in the blob"
            isLoading={ isLoading }
          >
            Commitment
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isLoading } overflow="hidden" whiteSpace="pre-wrap" wordBreak="break-all">
              { data.kzg_commitment }
              <CopyToClipboard text={ data.kzg_commitment } isLoading={ isLoading }/>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.blob_data && (
        <>
          <DetailedInfo.ItemLabel
            hint="Blob size in bytes"
            isLoading={ isLoading }
          >
            Size, bytes
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ isLoading } overflow="hidden" whiteSpace="pre-wrap" wordBreak="break-all">
              { (data.blob_data.replace('0x', '').length / 2).toLocaleString() }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { data.transaction_hashes[0] && (
        <>
          <DetailedInfo.ItemLabel
            hint="Hash of the transaction with this blob"
            isLoading={ isLoading }
          >
            Transaction hash
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <TxEntity hash={ data.transaction_hashes[0].transaction_hash } isLoading={ isLoading } noIcon/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfoSponsoredItem isLoading={ isLoading }/>

      { data.blob_data && (
        <BlobData data={ data.blob_data } hash={ data.hash } isLoading={ isLoading }/>
      ) }
    </DetailedInfo.Container>
  );
};

export default React.memo(BlobInfo);
