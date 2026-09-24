import { Flex } from '@chakra-ui/react';
import React from 'react';

import type {
  TokenTransfer,
  Erc20TotalPayload,
  Erc721TotalPayload,
  Erc1155TotalPayload,
  Erc404TotalPayload,
} from 'types/api/tokenTransfer';

import { isConfidentialTokenType } from 'lib/token/tokenTypes';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import ConfidentialTokenValue from 'ui/shared/value/ConfidentialTokenValue';

import TokenTransferSnippetFiat from './TokenTransferSnippetFiat';
import TokenTransferSnippetNft from './TokenTransferSnippetNft';

interface Props {
  data: TokenTransfer;
  noAddressIcons?: boolean;
  isLoading?: boolean;
}

const TokenTransferSnippet = ({ data, isLoading, noAddressIcons = true }: Props) => {

  const content = (() => {

    if (isLoading) {
      return <Skeleton loading w="250px" h={ 6 }/>;
    }

    if (data.token && isConfidentialTokenType(data.token.type)) {
      return <ConfidentialTokenValue token={ data.token } loading={ false }/>;
    }

    switch (data.token?.type) {
      case 'ERC-20': {
        const total = data.total as Erc20TotalPayload | null;
        if (total === null || total.value === null) {
          return null;
        }
        return <TokenTransferSnippetFiat token={ data.token } value={ total.value } decimals={ total.decimals }/>;
      }

      case 'ERC-721': {
        const total = data.total as Erc721TotalPayload;
        return (
          <TokenTransferSnippetNft
            token={ data.token }
            tokenId={ total.token_id }
            instance={ total.token_instance }
            value="1"
          />
        );
      }

      case 'ERC-1155': {
        const total = data.total as Erc1155TotalPayload;
        return (
          <TokenTransferSnippetNft
            key={ total.token_id }
            token={ data.token }
            tokenId={ total.token_id }
            instance={ total.token_instance }
            value={ total.value }
          />
        );
      }

      case 'ERC-404': {
        const total = data.total as Erc404TotalPayload | null;
        if (total === null) {
          return null;
        }

        if (total.token_id !== null) {
          return (
            <TokenTransferSnippetNft
              token={ data.token }
              tokenId={ total.token_id }
              value="1"
            />
          );
        } else {
          if (total.value === null) {
            return null;
          }

          return <TokenTransferSnippetFiat token={ data.token } value={ total.value } decimals={ total.decimals }/>;
        }
      }

      default: {
        return null;
      }
    }
  })();

  return (
    <Flex
      alignItems="center"
      flexWrap="wrap"
      columnGap={ 2 }
      rowGap={ 0 }
      flexDir="row"
      w="100%"
    >
      <AddressFromTo
        from={ data.from }
        to={ data.to }
        truncation="constant"
        noIcon={ noAddressIcons }
        isLoading={ isLoading }
        lineHeight={{ lg: '24px' }}
      />
      { content }
    </Flex>
  );
};

export default React.memo(TokenTransferSnippet);
