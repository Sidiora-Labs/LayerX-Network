import { Flex, Box } from '@chakra-ui/react';
import React from 'react';

import type { TokenInstance } from 'types/api/token';
import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { ClusterChainConfig } from 'types/multichain';

import { hasTokenTransferValue, isConfidentialTokenType, NFT_TOKEN_TYPE_IDS } from 'lib/token/tokenTypes';
import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import AssetValue from 'ui/shared/value/AssetValue';
import ConfidentialValue from 'ui/shared/value/ConfidentialValue';

type Props = TokenTransfer & { tokenId?: string; isLoading?: boolean; instance?: TokenInstance; chainData?: ClusterChainConfig };

const TokenTransferTableItem = ({
  token,
  total,
  transaction_hash: txHash,
  from,
  to,
  method,
  timestamp,
  tokenId,
  isLoading,
  instance,
  chainData,
}: Props) => {

  return (
    <TableRow alignItems="top">
      { chainData && (
        <TableCell>
          <ChainIcon data={ chainData } isLoading={ isLoading } my="5px"/>
        </TableCell>
      ) }
      <TableCell>
        <Flex flexDirection="column" alignItems="flex-start" mt="5px" rowGap={ 3 }>
          { txHash ? (
            <TxEntity
              hash={ txHash }
              isLoading={ isLoading }
              fontWeight={ 600 }
              noIcon
              truncation="constant_long"
            />
          ) : <Skeleton loading={ isLoading }>-</Skeleton> }
          <TimeWithTooltip
            timestamp={ timestamp }
            enableIncrement
            isLoading={ isLoading }
            display="inline-block"
            color="text.secondary"
            fontWeight="400"
          />
        </Flex>
      </TableCell>
      <TableCell>
        { method ? (
          <Box my="3px">
            <Badge loading={ isLoading } truncated>{ method }</Badge>
          </Box>
        ) : null }
      </TableCell>
      <TableCell>
        <AddressFromTo
          from={ from }
          to={ to }
          isLoading={ isLoading }
          mt="5px"
          mode={{ lg: 'compact', xl: 'long' }}
          tokenHash={ token?.address_hash }
          tokenSymbol={ token?.symbol ?? undefined }
        />
      </TableCell>
      { (token && NFT_TOKEN_TYPE_IDS.includes(token.type)) && (
        <TableCell>
          { total && 'token_id' in total && token && total.token_id !== null ? (
            <NftEntity
              hash={ token.address_hash }
              id={ total.token_id }
              instance={ instance || total.token_instance }
              noLink={ Boolean(tokenId && tokenId === total.token_id) }
              isLoading={ isLoading }
            />
          ) : ''
          }
        </TableCell>
      ) }
      { token && (hasTokenTransferValue(token.type)) && (
        <TableCell isNumeric verticalAlign="top">
          { isConfidentialTokenType(token.type) ? (
            <ConfidentialValue loading={ isLoading } mt="7px" wordBreak="break-all"/>
          ) : (
            <AssetValue
              amount={ total && 'value' in total ? total.value : null }
              decimals={ total && 'decimals' in total ? total.decimals || '0' : '0' }
              exchangeRate={ token?.exchange_rate }
              loading={ isLoading }
              layout="vertical"
              mt="7px"
              rowGap="10px"
            />
          ) }
        </TableCell>
      ) }
    </TableRow>
  );
};

export default React.memo(TokenTransferTableItem);
