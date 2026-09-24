import React from 'react';

import type { AdvancedFilterParams, AdvancedFiltersSearchParams } from 'types/api/advancedFilter';

import type { ColumnsIds } from 'ui/advancedFilter/constants';
import TableColumnFilterWrapper from 'ui/shared/filters/TableColumnFilterWrapper';

import { NATIVE_TOKEN } from './constants';
import type { AddressFilterMode } from './filters/AddressFilter';
import AddressFilter from './filters/AddressFilter';
import AddressRelationFilter from './filters/AddressRelationFilter';
import AgeFilter from './filters/AgeFilter';
import AmountFilter from './filters/AmountFilter';
import type { AssetFilterMode } from './filters/AssetFilter';
import AssetFilter from './filters/AssetFilter';
import MethodFilter from './filters/MethodFilter';
import TypeFilter from './filters/TypeFilter';

type Props = {
  filters: AdvancedFilterParams;
  searchParams?: AdvancedFiltersSearchParams;
  column: ColumnsIds;
  columnName: string;
  handleFilterChange: <T extends keyof AdvancedFilterParams>(field: T, val: AdvancedFilterParams[T]) => void;
  isLoading?: boolean;
};

const FilterByColumn = ({ column, filters, columnName, handleFilterChange, searchParams, isLoading }: Props) => {
  const commonProps = { columnName, handleFilterChange, isLoading };
  switch (column) {
    case 'type': {
      const value = filters.transaction_types;
      return (
        <TableColumnFilterWrapper
          columnName="Type"
          isLoading={ isLoading }
          selected={ Boolean(value && value.length) }
        >
          <TypeFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'method': {
      const value = filters.methods?.map(m => ({ name: searchParams?.methods[m], method_id: m }));
      return (
        <TableColumnFilterWrapper
          columnName="Method"
          isLoading={ isLoading }
          selected={ Boolean(value && value.length) }
          w="350px"
        >
          <MethodFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'age': {
      const value = { age: filters.age || '' as const, from: filters.age_from || '', to: filters.age_to || '' };
      return (
        <TableColumnFilterWrapper
          columnName="Age"
          isLoading={ isLoading }
          selected={ Boolean(value.from || value.to || value.age) }
          w="382px"
        >
          <AgeFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'or_and': {
      return (
        <TableColumnFilterWrapper
          columnName="And/Or"
          isLoading={ isLoading }
          selected
          w="106px"
          value={ filters.address_relation === 'and' ? 'AND' : 'OR' }
        >
          <AddressRelationFilter { ...commonProps } value={ filters.address_relation }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'from': {
      const valueInclude = filters?.from_address_hashes_to_include?.map(hash => ({ address: hash, mode: 'include' as AddressFilterMode }));
      const valueExclude = filters?.from_address_hashes_to_exclude?.map(hash => ({ address: hash, mode: 'exclude' as AddressFilterMode }));

      const value = (valueInclude || []).concat(valueExclude || []);
      return (
        <TableColumnFilterWrapper
          columnName="Address from"
          isLoading={ isLoading }
          selected={ Boolean(value.length) }
          w="480px"
        >
          <AddressFilter { ...commonProps } type="from" value={ value }/>
        </TableColumnFilterWrapper>
      );

    }
    case 'to': {
      const valueInclude = filters?.to_address_hashes_to_include?.map(hash => ({ address: hash, mode: 'include' as AddressFilterMode }));
      const valueExclude = filters?.to_address_hashes_to_exclude?.map(hash => ({ address: hash, mode: 'exclude' as AddressFilterMode }));

      const value = (valueInclude || []).concat(valueExclude || []);
      return (
        <TableColumnFilterWrapper
          columnName="Address to"
          isLoading={ isLoading }
          selected={ Boolean(value.length) }
          w="480px"
        >
          <AddressFilter { ...commonProps } type="to" value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'amount': {
      const value = { from: filters.amount_from, to: filters.amount_to };
      return (
        <TableColumnFilterWrapper
          columnName="Amount"
          isLoading={ isLoading }
          selected={ Boolean(value.from || value.to) }
          w="382px"
        >
          <AmountFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'asset': {
      const tokens = searchParams?.tokens;

      const value = tokens ?
        Object.entries(tokens).map(([ address, token ]) => {
          const mode = filters.token_contract_address_hashes_to_include?.find(i => i.toLowerCase() === address.toLowerCase()) ?
            'include' as AssetFilterMode :
            'exclude' as AssetFilterMode;
          return ({ token, mode });
        }) : [];
      if (filters.token_contract_address_hashes_to_include?.includes('native')) {
        value.unshift({ token: NATIVE_TOKEN, mode: 'include' });
      }
      if (filters.token_contract_address_hashes_to_exclude?.includes('native')) {
        value.unshift({ token: NATIVE_TOKEN, mode: 'exclude' });
      }
      return (
        <TableColumnFilterWrapper
          columnName="Asset"
          isLoading={ isLoading }
          selected={ Boolean(value.length) }
          w="382px"
        >
          <AssetFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    default: {
      return null;
    }
  }
};

export default FilterByColumn;
