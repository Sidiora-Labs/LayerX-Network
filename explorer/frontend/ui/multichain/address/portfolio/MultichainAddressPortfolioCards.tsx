import { type LinkProps } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import { clamp } from 'es-toolkit';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import multichainConfig from 'configs/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import { CollapsibleList } from 'toolkit/chakra/collapsible';
import IconSvg from 'ui/shared/IconSvg';

import MultichainAddressPortfolioCard from './MultichainAddressPortfolioCard';

const TRIGGER_PROPS: LinkProps = {
  variant: 'secondary',
  textStyle: { base: 'xs', lg: 'sm' },
  py: { base: 1, lg: '2px' },
};

interface Props {
  isLoading: boolean;
  selectedChainId: string | null;
  onChange: (chainId: string) => void;
  chainValues: multichain.AddressPortfolio['chain_values'];
  totalValue?: string;
}

const MultichainAddressPortfolioCards = ({ isLoading, selectedChainId, onChange, chainValues, totalValue }: Props) => {

  const [ initialActiveIndex, setInitialActiveIndex ] = React.useState<number | undefined>(undefined);

  const chains = multichainConfig()?.chains;

  const isMobile = useIsMobile();

  const items = React.useMemo(() => {
    const totalValueBn = BigNumber(totalValue ?? '0');
    const result: Array<{ chain: ClusterChainConfig; value: BigNumber; share: number | undefined }> = chainValues ?
      Object.entries(chainValues)
        .map(([ chainId, value ]) => {
          const chain = chains?.find((chain) => chain.id === chainId);
          if (!chain) {
            return null;
          }

          const valueBn = BigNumber(value);
          return {
            chain,
            value: valueBn,
            share: totalValueBn.gt(0) ? valueBn.div(totalValueBn).toNumber() : undefined,
          };
        })
        .filter(Boolean)
        .sort((a, b) => b.value.minus(a.value).toNumber()) :
      [];

    return result;
  }, [ chainValues, chains, totalValue ]);

  const cutLength = clamp(items.filter(({ share }) => share).length, 0, isMobile ? 6 : 10) || items.length;

  React.useEffect(() => {
    if (!isLoading && selectedChainId !== null) {
      const activeIndex = items.findIndex((item) => item.chain?.id === selectedChainId);
      setInitialActiveIndex((prev) => prev === undefined ? activeIndex : prev);
    }
  }, [ isLoading, items, selectedChainId ]);

  const renderItem = React.useCallback((item: typeof items[number]) => {
    return (
      <MultichainAddressPortfolioCard
        key={ item.chain.id }
        chain={ item.chain }
        value={ item.value }
        share={ item.share }
        isLoading={ isLoading }
        isSelected={ Boolean(item.chain) && selectedChainId === item.chain?.id }
        noneIsSelected={ selectedChainId === null }
        totalNum={ items.length }
        onClick={ items.length > 1 ? onChange : undefined }
      />
    );
  }, [ isLoading, selectedChainId, onChange, items ]);

  if (items.length === 0) {
    return null;
  }

  const text: [React.ReactNode, React.ReactNode] = [
    <>
      <IconSvg name="plus" boxSize={{ base: '8px', lg: '10px' }} mr={ 0.5 }/>
      <span>show more</span>
    </>,
    <>
      <IconSvg name="minus" boxSize={{ base: '8px', lg: '10px' }} mr={ 0.5 }/>
      <span>show less</span>
    </>,
  ];

  return (
    <CollapsibleList
      items={ items }
      renderItem={ renderItem }
      cutLength={ cutLength }
      text={ text }
      triggerProps={ TRIGGER_PROPS }
      defaultExpanded={ initialActiveIndex !== undefined ? initialActiveIndex > cutLength : undefined }
      flexDir="row"
      alignItems="center"
      gap={ 2 }
      flexWrap="wrap"
      mt={ 2 }
    />
  );
};

export default React.memo(MultichainAddressPortfolioCards);
