import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { TokenInfo, TokenInstance } from 'types/api/token';

import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';

interface Props {
  token: TokenInfo;
  value: string;
  tokenId: string | null;
  instance?: TokenInstance | null;
}

const NftTokenTransferSnippet = ({ value, token, tokenId, instance }: Props) => {
  const num = value === '1' ? '' : value;

  const tokenIdContent = (() => {
    if (tokenId === null) {
      // ERC-404 may not have an ID
      if (token.type === 'ERC-404') {
        return null;
      }
      return <chakra.span color="text.secondary"> N/A </chakra.span>;
    }

    return (
      <NftEntity
        hash={ token.address_hash }
        id={ tokenId }
        instance={ instance }
        variant="content"
        maxW={{ base: '100%', lg: '150px' }}
        w="auto"
        flexShrink={ 0 }
      />
    );

  })();

  return (
    <>
      { num ? (
        <>
          <chakra.span color="text.secondary">for</chakra.span>
          <span>{ num }</span>
          <chakra.span color="text.secondary">token ID</chakra.span>
        </>
      ) : (
        <chakra.span color="text.secondary">for token ID</chakra.span>
      ) }
      { tokenIdContent }
      <chakra.span color="text.secondary">of</chakra.span>
      <TokenEntity
        token={ token }
        noCopy
        w="auto"
        flexGrow={ 1 }
      />
    </>
  );
};

export default React.memo(NftTokenTransferSnippet);
