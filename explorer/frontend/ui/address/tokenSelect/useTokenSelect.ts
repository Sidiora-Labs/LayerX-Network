import { mapValues } from 'es-toolkit';
import React from 'react';

import type { FormattedData } from './types';

import type { Sort } from '../utils/tokenUtils';
import { filterTokens } from '../utils/tokenUtils';

export default function useTokenSelect(data: FormattedData) {
  const [ searchTerm, setSearchTerm ] = React.useState('');
  const [ erc1155sort, setErc1155Sort ] = React.useState<Sort>('desc');
  const [ erc404sort, setErc404Sort ] = React.useState<Sort>('desc');
  const [ fungibleSortByType, setFungibleSortByType ] = React.useState<Record<string, Sort>>({});

  const onInputChange = React.useCallback((searchTerm: string) => {
    setSearchTerm(searchTerm);
  }, []);

  const onSortClick = React.useCallback((event: React.SyntheticEvent) => {
    const tokenType = (event.currentTarget as HTMLAnchorElement).getAttribute('data-type');
    if (tokenType === 'ERC-1155') {
      setErc1155Sort((prevValue) => prevValue === 'desc' ? 'asc' : 'desc');
      return;
    }
    if (tokenType === 'ERC-404') {
      setErc404Sort((prevValue) => prevValue === 'desc' ? 'asc' : 'desc');
      return;
    }
    if (!tokenType) {
      return;
    }
    setFungibleSortByType((prevValue) => {
      const current = prevValue[tokenType] || 'desc';
      return {
        ...prevValue,
        [tokenType]: current === 'desc' ? 'asc' : 'desc',
      };
    });
  }, []);

  const filteredData = React.useMemo(() => {
    return mapValues(data, ({ items, isOverflow }) => ({
      isOverflow,
      items: items.filter(filterTokens(searchTerm.toLowerCase())),
    }));
  }, [ data, searchTerm ]);

  const getSort = React.useCallback((typeId: string): Sort => {
    if (typeId === 'ERC-1155') {
      return erc1155sort;
    }
    if (typeId === 'ERC-404') {
      return erc404sort;
    }
    return fungibleSortByType[typeId] || 'desc';
  }, [ erc1155sort, erc404sort, fungibleSortByType ]);

  return {
    searchTerm,
    getSort,
    erc1155sort,
    erc404sort,
    onInputChange,
    onSortClick,
    data,
    filteredData,
  };
}
