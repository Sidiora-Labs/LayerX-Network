import { pickBy } from 'es-toolkit';
import { useRouter } from 'next/router';
import React from 'react';

import type { MethodType, SmartContractMethod } from './types';

import getQueryParamString from 'lib/router/getQueryParamString';

import type { CONTRACT_MAIN_TAB_IDS } from '../utils';
import { TYPE_FILTER_OPTIONS, isReadMethod, isWriteMethod } from './utils';

function getInitialMethodType(tab: string) {
  switch (tab) {
    case 'read_contract':
    case 'read_proxy':
    case 'read_custom_methods':
      return 'read';
    case 'write_contract':
    case 'write_proxy':
    case 'write_custom_methods':
      return 'write';
    default:
      return 'all';
  }
}

const METHOD_TABS_MATRIX: Array<Array<typeof CONTRACT_MAIN_TAB_IDS[number]>> = [
  [ 'read_write_contract', 'read_contract', 'write_contract' ],
  [ 'read_write_proxy', 'read_proxy', 'write_proxy' ],
  [ 'read_write_custom_methods', 'read_custom_methods', 'write_custom_methods' ],
];

interface MethodFilterType {
  type: 'method_type';
  value: MethodType;
}

interface MethodFilterName {
  type: 'method_name';
  value: string;
}

export type MethodsFilters = MethodFilterType | MethodFilterName;

interface Params {
  abi: Array<SmartContractMethod>;
}

export default function useMethodsFilters({ abi }: Params) {
  const router = useRouter();

  const tab = getQueryParamString(router.query.tab);

  const [ methodType, setMethodType ] = React.useState<MethodType>(getInitialMethodType(tab));
  const [ searchTerm, setSearchTerm ] = React.useState<string>('');

  const changeTabInQuery = React.useCallback((methodType: MethodType) => {
    const currentTab = getQueryParamString(router.query.tab);
    const tabIndex = TYPE_FILTER_OPTIONS.findIndex(({ value }) => value === methodType);
    const nextTab = METHOD_TABS_MATRIX.find((tabsSet) => tabsSet.includes(currentTab))?.[tabIndex];

    if (!nextTab) {
      return;
    }

    const queryForPathname = pickBy(router.query, (value, key) => router.pathname.includes(`[${ key }]`));
    router.push(
      { pathname: router.pathname, query: { ...queryForPathname, tab: nextTab } },
      undefined,
      { shallow: true },
    );

  }, [ router ]);

  const onChange = React.useCallback((filters: MethodsFilters) => {
    if (filters.type === 'method_type') {
      setMethodType(filters.value);
      changeTabInQuery(filters.value);
    } else if (filters.type === 'method_name') {
      setSearchTerm(filters.value);
    }
  }, [ changeTabInQuery ]);

  return React.useMemo(() => {
    const typeFilterFn = (() => {
      switch (methodType) {
        case 'all':
          return () => true;
        case 'read':
          return isReadMethod;
        case 'write':
          return isWriteMethod;
      }
    })();

    const nameFilterFn = (method: SmartContractMethod) => {
      const searchTermLower = searchTerm.toLowerCase().trim();

      if (searchTermLower === '') {
        return true;
      }

      if (method.type === 'fallback') {
        return 'fallback'.includes(searchTermLower);
      }

      if (method.type === 'receive') {
        return 'receive'.includes(searchTermLower);
      }

      return method.name.toLowerCase().includes(searchTermLower);
    };

    return {
      methodType,
      searchTerm,
      onChange,
      visibleItems: abi
        .map((method, index) => typeFilterFn(method) && nameFilterFn(method) ? index : -1)
        .filter((item) => item !== -1),
    };
  }, [ methodType, searchTerm, onChange, abi ]);
}
