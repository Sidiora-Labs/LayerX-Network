import { HStack, VStack } from '@chakra-ui/react';
import type BigNumber from 'bignumber.js';
import { clamp } from 'es-toolkit';
import React from 'react';

import type { ClusterChainConfig } from 'types/multichain';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import SimpleValue from 'ui/shared/value/SimpleValue';
import { DEFAULT_ACCURACY_USD } from 'ui/shared/value/utils';

import { formatPercentage } from './utils';

interface Props {
  chain: ClusterChainConfig;
  value: BigNumber;
  share?: number;
  isLoading: boolean;
  isSelected: boolean;
  noneIsSelected: boolean;
  totalNum?: number;
  onClick?: (chainId: string) => void;
}

const MultichainAddressPortfolioCard = ({ chain, value, share, isLoading, isSelected: isSelectedProp, noneIsSelected, totalNum, onClick }: Props) => {

  const isSelected = isSelectedProp || totalNum === 1;

  const columnNumDesktop = clamp(totalNum || 0, 3, 5);
  const cardWidth = React.useMemo(() => {
    return {
      base: (totalNum || 0) > 1 ? 'calc((100% - 8px) / 2)' : '100%',
      lg: `calc((100% - ${ (columnNumDesktop - 1) * 8 }px) / ${ columnNumDesktop })`,
    };
  }, [ totalNum, columnNumDesktop ]);

  const handleClick = React.useCallback(() => {
    chain.id && onClick?.(chain.id);
  }, [ chain.id, onClick ]);

  return (
    <HStack
      p={ 3 }
      w={ cardWidth }
      borderRadius="base"
      border="1px solid"
      borderColor={ isSelected ? 'transparent' : 'border.divider' }
      textStyle="xs"
      bgColor={ isSelected ? 'selected.control.bg' : 'transparent' }
      opacity={ !isSelected && !noneIsSelected ? 0.5 : 1 }
      _hover={ onClick ? {
        borderColor: 'hover',
        opacity: 1,
      } : undefined }
      cursor={ onClick ? 'pointer' : 'default' }
      onClick={ handleClick }
      aria-label={ `${ chain.name } portfolio selector` }
    >
      <ChainIcon data={ chain } boxSize="30px" flexShrink={ 0 } isLoading={ isLoading } noTooltip/>
      <VStack alignItems="flex-start" gap={ 1 } overflow="hidden">
        <TruncatedText text={ chain.name } loading={ isLoading } color="text.secondary" maxW="100%"/>
        <HStack gap={ 1 } maxW="100%">
          <SimpleValue value={ value } prefix="$" loading={ isLoading } noTooltip accuracy={ DEFAULT_ACCURACY_USD }/>
          { share !== undefined && share > 0 && (
            <Skeleton loading={ isLoading } color="text.secondary" flexShrink={ 0 }>
              <span>{ formatPercentage(share) }</span>
            </Skeleton>
          ) }
        </HStack>
      </VStack>
    </HStack>
  );
};

export default React.memo(MultichainAddressPortfolioCard);
