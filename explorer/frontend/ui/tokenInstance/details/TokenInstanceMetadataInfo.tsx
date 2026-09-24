import { Grid, GridItem } from '@chakra-ui/react';
import React from 'react';

import type { TokenInstance } from 'types/api/token';
import type { MetadataAttributes } from 'types/client/token';

import parseMetadata from 'lib/token/parseMetadata';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';

import { useMetadataUpdateContext } from '../contexts/metadataUpdate';

interface Props {
  data?: TokenInstance;
  isLoading?: boolean;
}

interface ItemProps {
  data: MetadataAttributes;
  isLoading?: boolean;
}

const Item = ({ data, isLoading }: ItemProps) => {
  const value = (() => {
    if (data.value_type === 'URL') {
      return (
        <Link
          external
          whiteSpace="nowrap"
          display="inline-flex"
          alignItems="center"
          w="100%"
          overflow="hidden"
          href={ data.value }
          textStyle="sm"
          loading={ isLoading }
        >
          <TruncatedText text={ data.value } w="calc(100% - 16px)" loading={ isLoading }/>
        </Link>
      );
    }

    return <TruncatedText text={ data.value } fontSize="sm" w="100%" loading={ isLoading }/>;
  })();

  return (
    <GridItem
      bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
      borderRadius="md"
      px={ 4 }
      py={ 2 }
      display="flex"
      flexDir="column"
      alignItems="flex-start"
    >
      <TruncatedText
        text={ data.trait_type }
        textStyle="xs"
        w="100%"
        color="text.secondary"
        fontWeight={ 500 }
        mb={ 1 }
        loading={ isLoading }
      />
      { value }
    </GridItem>
  );
};

const TokenInstanceMetadataInfo = ({ data, isLoading: isLoadingProp }: Props) => {
  const { status: refetchStatus } = useMetadataUpdateContext() || {};

  const metadata = React.useMemo(() => parseMetadata(data?.metadata), [ data ]);

  const hasMetadata = metadata && Boolean((metadata.name || metadata.description || metadata.attributes));
  if (!hasMetadata) {
    return null;
  }

  const isLoading = isLoadingProp || refetchStatus === 'WAITING_FOR_RESPONSE';

  return (
    <>
      <DetailedInfo.ItemDivider/>
      { metadata?.name && (
        <>
          <DetailedInfo.ItemLabel
            hint="NFT name"
            isLoading={ isLoading }
          >
            Name
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue
            whiteSpace="normal"
            wordBreak="break-word"
          >
            <Skeleton loading={ isLoading }>
              { metadata.name }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }
      { metadata?.description && (
        <>
          <DetailedInfo.ItemLabel
            hint="NFT description"
            isLoading={ isLoading }
          >
            Description
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue
            whiteSpace="normal"
            wordBreak="break-word"
          >
            <Skeleton loading={ isLoading }>
              { metadata.description }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }
      { metadata?.attributes && metadata.attributes.length > 0 && (
        <>
          <DetailedInfo.ItemLabel
            hint="NFT attributes"
            isLoading={ isLoading }
          >
            Attributes
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Grid gap={ 2 } templateColumns="repeat(auto-fill,minmax(160px, 1fr))" w="100%" whiteSpace="normal">
              { metadata.attributes
                .filter((attribute) => attribute.value)
                .map((attribute, index) => <Item key={ index } data={ attribute } isLoading={ isLoading }/>) }
            </Grid>
          </DetailedInfo.ItemValue>
        </>
      ) }
    </>
  );
};

export default React.memo(TokenInstanceMetadataInfo);
