import type { AddressParam } from './addressParams';
import type { SmartContractLicenseType } from './contract';

export type VerifiedContractsLanguage = 'solidity' | 'vyper' | 'yul' | 'scilla' | 'stylus_rust' | 'geas';

export interface VerifiedContract {
  address: AddressParam;
  certified?: boolean;
  coin_balance: string;
  compiler_version: string | null;
  language: VerifiedContractsLanguage;
  has_constructor_args: boolean;
  optimization_enabled: boolean;
  transactions_count: number | null;
  verified_at: string;
  market_cap: string | null;
  license_type: SmartContractLicenseType | null;
  zk_compiler_version?: string;
}

export interface VerifiedContractsResponse {
  items: Array<VerifiedContract>;
  next_page_params: {
    items_count: string;
    smart_contract_id: string;
  } | null;
}

export type VerifiedContractsFilter = VerifiedContractsLanguage;

export interface VerifiedContractsFilters {
  q: string | undefined;
  filter: VerifiedContractsFilter | undefined;
}

export type VerifiedContractsCounters = {
  new_smart_contracts_24h: string;
  new_verified_smart_contracts_24h: string;
  smart_contracts: string;
  verified_smart_contracts: string;
};

export interface HotContract {
  contract_address: AddressParam ;
  balance: string;
  transactions_count: string;
  total_gas_used: string;
}

export interface HotContractsResponse {
  items: Array<HotContract>;
  next_page_params: {
    items_count: string;
    transactions_count: string;
    total_gas_used: string;
    contract_address_hash: string;
  } | null;
}

export interface HotContractsFilters {
  scale?: HotContractsInterval;
}

export interface HotContractsSorting {
  sort: 'transactions_count' | 'total_gas_used';
  order: 'asc' | 'desc';
}

export type HotContractsSortingField = HotContractsSorting['sort'];

export type HotContractsSortingValue = `${ HotContractsSortingField }-${ HotContractsSorting['order'] }` | 'default';

export type HotContractsInterval = '5m' | '1h' | '3h' | '1d' | '7d' | '30d';
