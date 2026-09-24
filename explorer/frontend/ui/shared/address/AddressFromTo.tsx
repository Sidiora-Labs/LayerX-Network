import type { ConditionalValue } from '@chakra-ui/react';
import { Flex, Grid, chakra, useBreakpointValue } from '@chakra-ui/react';
import React from 'react';

import type { EntityProps } from 'ui/shared/entities/address/AddressEntity';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import AddressEntityWithTokenFilter from 'ui/shared/entities/address/AddressEntityWithTokenFilter';

import AddressEntityZetaChain from '../entities/address/AddressEntityZetaChain';
import AddressFromToIcon from './AddressFromToIcon';
import { getTxCourseType } from './utils';

type Mode = 'compact' | 'long';

interface Props {
  from: { hash: string } | { hash: string; chainId: string; chainType: 'zeta' };
  to: { hash: string } | { hash: string; chainId: string; chainType: 'zeta' } | null;
  current?: string;
  mode?: Mode | ConditionalValue<Mode>;
  className?: string;
  isLoading?: boolean;
  tokenHash?: string;
  tokenSymbol?: string;
  truncation?: EntityProps['truncation'];
  noIcon?: boolean;
}

const AddressFromTo = ({
  from,
  to,
  current,
  mode: modeProp,
  className, isLoading, tokenHash = '', tokenSymbol = '', noIcon }: Props) => {
  const mode = useBreakpointValue(
    {
      base: (typeof modeProp === 'object' && 'base' in modeProp ? modeProp.base : modeProp),
      lg: (typeof modeProp === 'object' && 'lg' in modeProp ? modeProp.lg : modeProp),
      xl: (typeof modeProp === 'object' && 'xl' in modeProp ? modeProp.xl : modeProp),
    },
  ) ?? 'long';

  const EntityFrom = (() => {
    if ('chainType' in from && from.chainType === 'zeta') {
      return AddressEntityZetaChain;
    }
    if (tokenHash && tokenSymbol) {
      return AddressEntityWithTokenFilter;
    }
    return AddressEntity;
  })();

  const EntityTo = (() => {
    if (to && 'chainType' in to && to.chainType === 'zeta') {
      return AddressEntityZetaChain;
    }
    if (tokenHash && tokenSymbol) {
      return AddressEntityWithTokenFilter;
    }
    return AddressEntity;
  })();

  const isOutgoing = current ? current.toLowerCase() === from.hash.toLowerCase() : false;
  const isIncoming = current ? current.toLowerCase() === to?.hash?.toLowerCase() : false;

  const fromChainId = 'chainType' in from && from.chainType === 'zeta' ? from.chainId : undefined;
  const toChainId = to && 'chainType' in to && to.chainType === 'zeta' ? to.chainId : undefined;

  if (mode === 'compact') {
    return (
      <Flex className={ className } flexDir="column" rowGap={ 3 }>
        <Flex alignItems="center" columnGap={ 2 }>
          <AddressFromToIcon
            isLoading={ isLoading }
            type={ getTxCourseType(from.hash, to?.hash, current) }
            transform="rotate(90deg)"
          />
          <EntityFrom
            address={ from }
            isLoading={ isLoading }
            noLink={ isOutgoing }
            noCopy={ isOutgoing }
            noIcon={ noIcon }
            tokenHash={ tokenHash }
            tokenSymbol={ tokenSymbol }
            truncation="constant"
            maxW="calc(100% - 28px)"
            w="min-content"
            chainId={ fromChainId }
          />
        </Flex>
        { to && (
          <EntityTo
            address={ to }
            isLoading={ isLoading }
            noLink={ isIncoming }
            noCopy={ isIncoming }
            noIcon={ noIcon }
            tokenHash={ tokenHash }
            tokenSymbol={ tokenSymbol }
            truncation="constant"
            maxW="calc(100% - 28px)"
            w="min-content"
            ml="28px"
            chainId={ toChainId }
          />
        ) }
      </Flex>
    );
  }

  const iconSize = 20;

  return (
    <Grid className={ className } alignItems="center" gridTemplateColumns={ `minmax(auto, min-content) ${ iconSize }px minmax(auto, min-content)` }>
      <EntityFrom
        address={ from }
        isLoading={ isLoading }
        noLink={ isOutgoing }
        noCopy={ isOutgoing }
        noIcon={ noIcon }
        tokenHash={ tokenHash }
        tokenSymbol={ tokenSymbol }
        truncation="constant"
        mr={ isOutgoing ? 4 : 2 }
        chainId={ fromChainId }
        w="auto"
      />
      <AddressFromToIcon
        isLoading={ isLoading }
        type={ getTxCourseType(from.hash, to?.hash, current) }
      />
      { to && (
        <EntityTo
          address={ to }
          isLoading={ isLoading }
          noLink={ isIncoming }
          noCopy={ isIncoming }
          noIcon={ noIcon }
          tokenHash={ tokenHash }
          tokenSymbol={ tokenSymbol }
          truncation="constant"
          ml={ 3 }
          chainId={ toChainId }
          w="auto"
        />
      ) }
    </Grid>
  );
};

export default chakra(AddressFromTo);
