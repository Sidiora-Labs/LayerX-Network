import { castArray } from 'es-toolkit/compat';
import React from 'react';

import type { AdvancedFilterAge } from 'types/api/advancedFilter';
import type { TokenInfo } from 'types/api/token';
import { ZETA_CHAIN_CCTX_COIN_TYPE_FILTER, type ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import TableColumnFilterWrapper from 'ui/shared/filters/TableColumnFilterWrapper';

import ZetaChainAgeFilter from './ZetaChainAgeFilter';
import ZetaChainAssetFilter, { ZETA_NATIVE_TOKEN } from './ZetaChainAssetFilter';
import ZetaChainReceiverFilter from './ZetaChainReceiverFilter';
import ZetaChainSenderFilter from './ZetaChainSenderFilter';
import ZetaChainStatusFilter from './ZetaChainStatusFilter';

type ColumnId = 'age' | 'status' | 'sender' | 'receiver' | 'asset';

type Props = {
  filters: ZetaChainCCTXFilterParams;
  column: ColumnId;
  columnName: string;
  handleFilterChange: <T extends keyof ZetaChainCCTXFilterParams>(field: T, val: ZetaChainCCTXFilterParams[T]) => void;
  isLoading?: boolean;
};

const ZetaChainFilterByColumn = ({ column, filters, columnName, handleFilterChange, isLoading }: Props) => {
  const commonProps = { columnName, handleFilterChange, isLoading };

  switch (column) {
    case 'age': {
      const value = { age: (filters.age || '') as AdvancedFilterAge | '', from: filters.start_timestamp || '', to: filters.end_timestamp || '' };
      return (
        <TableColumnFilterWrapper
          columnName="Age"
          isLoading={ isLoading }
          selected={ Boolean(value.age || value.from || value.to) }
          w="382px"
        >
          <ZetaChainAgeFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'status': {
      const value = filters.status_reduced ? castArray(filters.status_reduced) : [];
      return (
        <TableColumnFilterWrapper
          columnName="Status"
          isLoading={ isLoading }
          selected={ Boolean(value && value.length) }
          w="200px"
        >
          <ZetaChainStatusFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'sender': {
      const value = filters.sender_address ? castArray(filters.sender_address) : [];
      const chainValue = filters.source_chain_id ? castArray(filters.source_chain_id) : [];
      return (
        <TableColumnFilterWrapper
          columnName="Sender"
          isLoading={ isLoading }
          selected={ Boolean(value && value.length) || Boolean(chainValue && chainValue.length) }
          w="480px"
        >
          <ZetaChainSenderFilter { ...commonProps } value={ value } chainValue={ chainValue }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'receiver': {
      const value = filters.receiver_address ? castArray(filters.receiver_address) : [];
      const chainValue = filters.target_chain_id ? castArray(filters.target_chain_id) : [];
      return (
        <TableColumnFilterWrapper
          columnName="Receiver"
          isLoading={ isLoading }
          selected={ Boolean(value && value.length) || Boolean(chainValue && chainValue.length) }
          w="480px"
        >
          <ZetaChainReceiverFilter { ...commonProps } value={ value } chainValue={ chainValue }/>
        </TableColumnFilterWrapper>
      );
    }
    case 'asset': {
      let value: TokenInfo | null = null;
      if (filters.coin_type && castArray(filters.coin_type).includes(ZETA_CHAIN_CCTX_COIN_TYPE_FILTER)) {
        value = ZETA_NATIVE_TOKEN;
      }
      if (filters.token_symbol?.[0]) {
        value = {
          address_hash: '',
          symbol: filters.token_symbol[0],
          name: filters.token_symbol[0],
          decimals: '18',
          total_supply: '0',
          icon_url: null,
          type: 'ERC-20',
          holders_count: null,
          exchange_rate: null,
          circulating_market_cap: null,
          reputation: null,
        };
      }

      return (
        <TableColumnFilterWrapper
          columnName="Asset"
          isLoading={ isLoading }
          selected={ Boolean(value) }
          w="350px"
        >
          <ZetaChainAssetFilter { ...commonProps } value={ value }/>
        </TableColumnFilterWrapper>
      );
    }
    default:
      return null;
  }
};

export default ZetaChainFilterByColumn;
