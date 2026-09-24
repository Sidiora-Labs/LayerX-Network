import type { TokenInfo } from 'types/api/token';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { getTokenTypes } from 'lib/token/tokenTypes';

export type ColumnsIds = 'tx_hash' | 'type' | 'method' | 'age' | 'from' | 'or_and' | 'to' | 'amount' | 'asset' | 'fee';

type TxTableColumn = {
  id: ColumnsIds;
  name: string;
  width: string;
  isNumeric?: boolean;
};

export const TABLE_COLUMNS: Array<TxTableColumn> = [
  {
    id: 'tx_hash',
    name: 'Tx hash',
    width: '180px',
  },
  {
    id: 'type',
    name: 'Type',
    width: '160px',
  },
  {
    id: 'method',
    name: 'Method',
    width: '160px',
  },
  {
    id: 'age',
    name: 'Age',
    width: '190px',
  },
  {
    id: 'from',
    name: 'From',
    width: '160px',
  },
  {
    id: 'or_and',
    name: '',
    width: '65px',
  },
  {
    id: 'to',
    name: 'To',
    width: '160px',
  },
  {
    id: 'amount',
    name: 'Amount',
    isNumeric: true,
    width: '150px',
  },
  {
    id: 'asset',
    name: 'Asset',
    width: '120px',
  },
  {
    id: 'fee',
    name: 'Fee',
    isNumeric: true,
    width: '120px',
  },
] as const;

export const getAdvancedFilterTypes = (chainConfig?: Array<ClusterChainConfig['app_config']> | ClusterChainConfig['app_config'], withAll = false) => {
  return [
    ...(withAll ? [ {
      id: 'all',
      name: 'All',
    } ] : []),
    {
      id: 'coin_transfer',
      name: 'Coin Transfer',
    },
    ...Object.entries(getTokenTypes(false, chainConfig))
      .map(([ id, name ]) => ({
        id,
        name: `${ name } Transfer`,
      })),
    {
      id: 'contract_creation',
      name: 'Contract Creation',
    },
    {
      id: 'contract_interaction',
      name: 'Contract Interaction',
    },
  ];
};

export const NATIVE_TOKEN = {
  name: config.chain.currency.name || '',
  icon_url: '',
  symbol: config.chain.currency.symbol || '',
  address_hash: 'native',
  type: 'ERC-20' as const,
} as TokenInfo;
