import { Box, Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { TokenInfo, TokenInstance } from 'types/api/token';

import { route } from 'nextjs/routes';

import { useMultichainContext } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedTextTooltip } from 'toolkit/components/truncation/TruncatedTextTooltip';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import NftMedia from 'ui/shared/nft/NftMedia';

type Props = { item: TokenInstance; token: TokenInfo; isLoading: boolean };

const TokenInventoryItem = ({ item, token, isLoading }: Props) => {

  const isMobile = useIsMobile();
  const multichainContext = useMultichainContext();

  const mediaElement = (
    <NftMedia
      mb="18px"
      data={ item }
      isLoading={ isLoading }
      autoplayVideo={ false }
      size="md"
    />
  );

  const url = route({ pathname: '/token/[hash]/instance/[id]', query: { hash: token.address_hash, id: item.id } }, multichainContext);

  return (
    <Box
      w={{ base: '100%', lg: '210px' }}
      border="1px solid"
      borderColor={{ _light: 'blackAlpha.100', _dark: 'whiteAlpha.200' }}
      borderRadius="12px"
      p="10px"
      textStyle="sm"
      fontWeight={ 500 }
    >
      <Link href={ isLoading ? undefined : url } display="inline">
        { mediaElement }
      </Link>
      { item.id && (
        <Flex mb={ 2 } ml={ 1 }>
          <Text whiteSpace="pre" color="text.secondary">ID# </Text>
          <TruncatedTextTooltip label={ item.id }>
            <Skeleton loading={ isLoading } overflow="hidden">
              <Link
                overflow="hidden"
                textOverflow="ellipsis"
                whiteSpace="nowrap"
                display="block"
                loading={ isLoading }
                href={ url }
              >
                { item.id }
              </Link>
            </Skeleton>
          </TruncatedTextTooltip>
        </Flex>
      ) }
      { item.owner && (
        <Flex mb={ 2 } ml={ 1 }>
          <Text whiteSpace="pre" color="text.secondary" mr={ 2 } lineHeight="24px">Owner</Text>
          <AddressEntity
            address={ item.owner }
            isLoading={ isLoading }
            truncation="constant"
            noCopy
            noIcon={ isMobile }
          />
        </Flex>
      ) }
    </Box>
  );
};

export default TokenInventoryItem;
