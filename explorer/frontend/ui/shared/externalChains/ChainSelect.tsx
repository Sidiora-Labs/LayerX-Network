import { Box, createListCollection, Separator } from '@chakra-ui/react';
import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import useIsMobile from 'lib/hooks/useIsMobile';
import type { SelectOption, SelectProps, ViewMode } from 'toolkit/chakra/select';
import { Select } from 'toolkit/chakra/select';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import IconSvg from 'ui/shared/IconSvg';

import ChainIcon from './ChainIcon';

const ALL_OPTION: SelectOption = {
  value: 'all',
  label: 'All chains',
  icon: <IconSvg name="pie_chart" boxSize={ 5 }/>,
  afterElement: <Separator orientation="horizontal" w="full"/>,
};

export interface Props extends Omit<SelectProps, 'collection' | 'placeholder'> {
  loading?: boolean;
  mode?: ViewMode;
  chainsConfig: Array<Omit<ExternalChain, 'explorer_url'>>;
  chainIds?: Array<string>;
  withAllOption?: boolean;
}

const ChainSelect = ({ loading, mode, chainsConfig, chainIds, withAllOption, ...props }: Props) => {

  const [ inputValue, setInputValue ] = React.useState('');

  const isInitialLoading = useIsInitialLoading(loading);
  const isMobile = useIsMobile();

  const allItems = React.useMemo(() => {
    const chainItems = chainsConfig
      .filter((chain) => !chainIds || isInitialLoading || chainIds.includes(chain.id))
      .map((chain) => ({
        value: chain.id,
        label: chain.name || `Chain ${ chain.id }`,
        icon: <ChainIcon data={ chain } alt={ `${ chain.name } logo` } borderRadius="none" noTooltip/>,
      })) || [];

    return [ withAllOption ? ALL_OPTION : undefined, ...chainItems ].filter(Boolean);
  }, [ chainsConfig, chainIds, withAllOption, isInitialLoading ]);

  const collection = React.useMemo(() => {
    return createListCollection<SelectOption>({ items: allItems });
  }, [ allItems ]);

  const handleFilterChange = React.useCallback((value: string) => {
    setInputValue(value);
  }, [ ]);

  const itemFilter = React.useCallback((item: SelectOption) => {
    return item.label.toLowerCase().includes(inputValue.toLowerCase());
  }, [ inputValue ]);

  const contentHeader = allItems.length > 10 ? (
    <Box px="4" pt={ 4 } pb={ 2 } position="sticky" top={ 0 } zIndex={ 1 } bgColor="popover.bg">
      <FilterInput
        placeholder="Find chain"
        initialValue={ inputValue }
        onChange={ handleFilterChange }
      />
    </Box>
  ) : null;

  if (allItems.length === 0) {
    return null;
  }

  return (
    <Select
      collection={ collection }
      defaultValue={ allItems.length > 0 ? [ allItems[0].value ] : undefined }
      placeholder="Select chain"
      loading={ isInitialLoading }
      mode={ isMobile && !mode ? 'compact' : mode }
      w="fit-content"
      flexShrink={ 0 }
      contentHeader={ contentHeader }
      contentProps={ contentHeader ? { pt: 0 } : undefined }
      itemFilter={ itemFilter }
      { ...props }
    />
  );
};

export default React.memo(ChainSelect);
