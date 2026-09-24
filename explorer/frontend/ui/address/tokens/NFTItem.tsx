import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { AddressNFT } from 'types/api/address';
import type { ClusterChainConfig } from 'types/multichain';

import { route } from 'nextjs/routes';

import { getTokenTypeName } from 'lib/token/tokenTypes';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tag } from 'toolkit/chakra/tag';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import NftMedia from 'ui/shared/nft/NftMedia';
import calculateUsdValue from 'ui/shared/value/calculateUsdValue';

import NFTItemContainer from './NFTItemContainer';

type Props = AddressNFT & { isLoading: boolean; withTokenLink?: boolean; chain?: ClusterChainConfig };

const NFTItem = ({ value, isLoading, withTokenLink, chain, ...tokenInstance }: Props) => {
  const { token } = tokenInstance;
  const valueResult = token.decimals && value ? calculateUsdValue({ amount: value, decimals: token.decimals, accuracy: 2 }).valueStr : value;
  const tokenInstanceLink = tokenInstance.id ?
    route({ pathname: '/token/[hash]/instance/[id]', query: { hash: token.address_hash, id: tokenInstance.id } }, chain ? { chain } : undefined) :
    undefined;

  return (
    <NFTItemContainer position="relative">
      <Skeleton loading={ isLoading } className="light">
        <Tag background="gray.50" zIndex={ 1 } position="absolute" top="18px" right="18px">{ getTokenTypeName(token.type) }</Tag>
      </Skeleton>
      <Link href={ isLoading ? undefined : tokenInstanceLink } display="inline">
        <NftMedia
          mb="18px"
          data={ tokenInstance }
          size="md"
          isLoading={ isLoading }
          autoplayVideo={ false }
        />
      </Link>
      <Flex justifyContent="space-between" w="100%" flexWrap="wrap">
        <Flex ml={ 1 } overflow="hidden">
          <Text whiteSpace="pre" color="text.secondary">ID# </Text>
          <NftEntity hash={ token.address_hash } id={ tokenInstance.id } isLoading={ isLoading } noIcon/>
        </Flex>
        <Skeleton loading={ isLoading } overflow="hidden" ml={ 1 }>
          { valueResult && (
            <Flex>
              <Text color="text.secondary" whiteSpace="pre">Qty </Text>
              <Text overflow="hidden" wordBreak="break-all">{ valueResult }</Text>
            </Flex>
          ) }
        </Skeleton>
      </Flex>
      { withTokenLink && (
        <TokenEntity
          mt={ 2 }
          token={ token }
          isLoading={ isLoading }
          noCopy
          noSymbol
          chain={ chain }
        />
      ) }
    </NFTItemContainer>
  );
};

export default NFTItem;
