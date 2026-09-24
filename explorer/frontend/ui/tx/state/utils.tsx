import { Flex } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { TxStateChange } from 'types/api/txStateChanges';

import config from 'configs/app';
import getNetworkValidatorTitle from 'lib/networks/getNetworkValidatorTitle';
import { currencyUnits } from 'lib/units';
import { Badge } from 'toolkit/chakra/badge';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { ZERO_ADDRESS } from 'toolkit/utils/consts';
import { nbsp, space } from 'toolkit/utils/htmlEntities';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';

import TxStateTokenIdList from './TxStateTokenIdList';

export function getStateElements(data: TxStateChange, isLoading?: boolean) {
  const tag = (() => {
    if (data.is_miner) {
      return (
        <Tooltip content="A block producer who successfully included the block into the blockchain">
          <Badge textTransform="capitalize" colorPalette="yellow" loading={ isLoading }>
            { getNetworkValidatorTitle() }
          </Badge>
        </Tooltip>
      );
    }

    if (data.address.hash === ZERO_ADDRESS) {
      const changeDirection = (() => {
        if (Array.isArray(data.change)) {
          const firstChange = data.change[0];
          return firstChange.direction;
        }
        return Number(data.change) > 0 ? 'to' : 'from';
      })();

      if (changeDirection) {
        const text = changeDirection === 'from' ? 'Mint' : 'Burn';
        return (
          <Tooltip content="Address used in tokens mintings and burnings">
            <Badge textTransform="capitalize" colorPalette="yellow" loading={ isLoading }>{ text } address</Badge>
          </Tooltip>
        );
      }
    }

    return null;
  })();

  switch (data.type) {
    case 'coin': {
      const beforeBn = BigNumber(data.balance_before || '0').div(10 ** config.chain.currency.decimals);
      const afterBn = BigNumber(data.balance_after || '0').div(10 ** config.chain.currency.decimals);
      const differenceBn = afterBn.minus(beforeBn);
      const changeColor = beforeBn.lte(afterBn) ? 'green.500' : 'red.500';
      const changeSign = beforeBn.lte(afterBn) ? '+' : '-';

      return {
        before: (
          <Skeleton loading={ isLoading } wordBreak="break-all" display="inline-block">
            { beforeBn.toFormat() } { currencyUnits.ether }
          </Skeleton>
        ),
        after: (
          <Skeleton loading={ isLoading } wordBreak="break-all" display="inline-block">
            { afterBn.toFormat() } { currencyUnits.ether }
          </Skeleton>
        ),
        change: (
          <Skeleton loading={ isLoading } display="inline-block" color={ changeColor }>
            <span>{ changeSign }{ nbsp }{ differenceBn.abs().toFormat() }</span>
          </Skeleton>
        ),
        tag,
      };
    }
    case 'token': {
      const tokenLink = (
        <TokenEntity
          token={ data.token }
          isLoading={ isLoading }
          noIcon
          noCopy
          onlySymbol
          w="auto"
        />
      );
      const beforeBn = BigNumber(data.balance_before || '0').div(BigNumber(10 ** (Number(data.token.decimals))));
      const afterBn = BigNumber(data.balance_after || '0').div(BigNumber(10 ** (Number(data.token.decimals))));
      const change = (() => {
        let differenceBn;
        if (typeof data.change === 'string') {
          differenceBn = BigNumber(data.change || '0').div(BigNumber(10 ** (Number(data.token.decimals))));
        } else {
          differenceBn = afterBn.minus(beforeBn);
        }

        if (!differenceBn || differenceBn.isEqualTo(0)) {
          return null;
        }

        const changeColor = differenceBn.isGreaterThanOrEqualTo(0) ? 'green.500' : 'red.500';
        const changeSign = differenceBn.isGreaterThanOrEqualTo(0) ? '+' : '-';

        return (
          <Skeleton loading={ isLoading } display="inline-block" color={ changeColor } wordBreak="break-all">
            <span>{ changeSign }{ nbsp }{ differenceBn.abs().toFormat() }</span>
          </Skeleton>
        );
      })();

      const tokenId = (() => {
        if (!Array.isArray(data.change)) {
          if ('token_id' in data && data.token_id) {
            return (
              <NftEntity
                hash={ data.token.address_hash }
                id={ data.token_id }
                isLoading={ isLoading }
              />
            );
          } else {
            return null;
          }
        }

        return <TxStateTokenIdList items={ data.change } tokenAddress={ data.token.address_hash } isLoading={ isLoading }/>;
      })();

      return {
        before: data.balance_before ? (
          <Flex whiteSpace="pre-wrap" justifyContent={{ base: 'flex-start', lg: 'flex-end' }} flexWrap="wrap">
            <Skeleton loading={ isLoading } wordBreak="break-all">{ beforeBn.toFormat() }</Skeleton>
            <span>{ space }</span>
            { tokenLink }
          </Flex>
        ) : null,
        after: data.balance_after ? (
          <Flex whiteSpace="pre-wrap" justifyContent={{ base: 'flex-start', lg: 'flex-end' }} flexWrap="wrap">
            <Skeleton loading={ isLoading } wordBreak="break-all">{ afterBn.toFormat() }</Skeleton>
            <span>{ space }</span>
            { tokenLink }
          </Flex>
        ) : null,
        change,
        tag,
        tokenId,
      };
    }
  }
}
