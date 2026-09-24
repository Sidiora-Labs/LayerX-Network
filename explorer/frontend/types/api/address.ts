import type { Transaction } from 'types/api/transaction';

import type { UserTags, AddressImplementation, AddressParam, AddressFilecoinParams } from './addressParams';
import type { Block } from './block';
import type { SmartContractCreationStatus, SmartContractProxyType } from './contract';
import type { CeloEpochRewardsType } from './epochs';
import type { InternalTransaction } from './internalTransaction';
import type { MudWorldSchema, MudWorldTable } from './mudWorlds';
import type { NFTTokenType, TokenInfo, TokenInstance, TokenType } from './token';
import type { TokenTransfer, TokenTransferPagination } from './tokenTransfer';

export interface Address extends UserTags {
  block_number_balance_updated_at: number | null;
  coin_balance: string | null;
  creator_address_hash: string | null;
  creator_filecoin_robust_address?: string | null;
  creation_transaction_hash: string | null;
  creation_status: SmartContractCreationStatus | null;
  exchange_rate: string | null;
  ens_domain_name: string | null;
  filecoin?: AddressFilecoinParams;
  zilliqa?: AddressZilliqaParams;
  celo?: AddressCeloParams;
  // TODO: if we are happy with tabs-counters method, should we delete has_something fields?
  has_beacon_chain_withdrawals?: boolean;
  has_logs: boolean;
  has_token_transfers: boolean;
  has_tokens: boolean;
  has_validated_blocks: boolean;
  hash: string;
  implementations: Array<AddressImplementation> | null;
  is_contract: boolean;
  is_verified: boolean;
  name: string | null;
  token: TokenInfo | null;
  watchlist_address_id: number | null;
  proxy_type?: SmartContractProxyType | null;
}

export interface AddressZilliqaParams {
  is_scilla_contract: boolean;
}

export interface AddressCeloParams {
  account: {
    locked_celo: string;
    metadata_url: string | null;
    name: string | null;
    nonvoting_locked_celo: string;
    type: string;
    vote_signer_address: AddressParam | null;
    validator_signer_address: AddressParam | null;
    attestation_signer_address: AddressParam | null;
  } | null;
}

export interface AddressCounters {
  transactions_count: string;
  token_transfers_count: string;
  gas_usage_count: string | null;
  validations_count: string | null;
}

export interface AddressTokenBalance {
  token: TokenInfo;
  token_id: string | null;
  value: string;
  token_instance: TokenInstance | null;
}
export type AddressTokenBalancesResponse = Array<AddressTokenBalance>;

export type AddressNFT = TokenInstance & {
  token: TokenInfo;
  token_type: Omit<TokenType, 'ERC-20'>;
  value: string;
};

export type AddressCollection = {
  token: TokenInfo;
  amount: string;
  token_instances: Array<Omit<AddressNFT, 'token'>>;
};

export interface AddressTokensResponse {
  items: Array<AddressTokenBalance>;
  next_page_params: {
    items_count: number;
    token_name: string | null;
    token_type: TokenType;
    value: number;
    fiat_value: string | null;
  } | null;
}

export interface AddressNFTsResponse {
  items: Array<AddressNFT>;
  next_page_params: {
    items_count: number;
    token_id: string;
    token_type: TokenType;
    token_contract_address_hash: string;
  } | null;
}

export interface AddressCollectionsResponse {
  items: Array<AddressCollection>;
  next_page_params: {
    token_contract_address_hash: string;
    token_type: TokenType;
  } | null;
}

export interface AddressTokensBalancesSocketMessage {
  overflow: boolean;
  token_balances: Array<AddressTokenBalance>;
}

export interface AddressTransactionsResponse {
  items: Array<Transaction>;
  next_page_params: {
    block_number: number;
    index: number;
    items_count: number;
  } | null;
}

export const AddressFromToFilterValues = [ 'from', 'to' ] as const;

export type AddressFromToFilter = typeof AddressFromToFilterValues[number] | undefined;

export type AddressTxsFilters = {
  filter: AddressFromToFilter;
};

export interface AddressTokenTransferResponse {
  items: Array<TokenTransfer>;
  next_page_params: TokenTransferPagination | null;
}

export type AddressTokenTransferFilters = {
  filter?: AddressFromToFilter;
  type?: Array<TokenType>;
  token?: string;
};

export type AddressTokensFilter = {
  type: TokenType | Array<TokenType>;
};

export type AddressNFTTokensFilter = {
  type: Array<NFTTokenType> | undefined;
};

export interface AddressCoinBalanceHistoryItem {
  block_number: number;
  block_timestamp: string;
  delta: string;
  transaction_hash: string | null;
  value: string;
}

export interface AddressCoinBalanceHistoryResponse {
  items: Array<AddressCoinBalanceHistoryItem>;
  next_page_params: {
    block_number: number;
    items_count: number;
  } | null;
}

export type AddressCoinBalanceHistoryChart = {
  items: Array<{
    date: string;
    value: string;
  }>;
  days: number;
};

export interface AddressBlocksValidatedResponse {
  items: Array<Block>;
  next_page_params: {
    block_number: number;
    items_count: number;
  };
}
export interface AddressInternalTxsResponse {
  items: Array<InternalTransaction>;
  next_page_params: {
    block_number: number;
    index: number;
    items_count: number;
    transaction_index: number;
  } | null;
}

export type AddressWithdrawalsResponse = {
  items: Array<AddressWithdrawalsItem>;
  next_page_params: {
    index: number;
    items_count: number;
  };
};

export type AddressWithdrawalsItem = {
  amount: string;
  block_number: number;
  index: number;
  timestamp: string;
  validator_index: number;
};

export type AddressTabsCounters = {
  internal_transactions_count: number | null;
  logs_count: number | null;
  token_balances_count: number | null;
  token_transfers_count: number | null;
  transactions_count: number | null;
  validations_count: number | null;
  withdrawals_count: number | null;
  beacon_deposits_count: number | null;
  celo_election_rewards_count?: number | null;
};

// MUD framework
export type AddressMudTableItem = {
  schema: MudWorldSchema;
  table: MudWorldTable;
};

export type AddressMudTables = {
  items: Array<AddressMudTableItem>;
  next_page_params: {
    items_count: number;
    table_id: string;
  };
};

export type AddressMudTablesFilter = {
  q?: string;
};

export type AddressMudRecords = {
  items: Array<AddressMudRecordsItem>;
  schema: MudWorldSchema;
  table: MudWorldTable;
  next_page_params: {
    items_count: number;
    key0: string;
    key1: string;
    key_bytes: string;
  };
};

export type AddressMudRecordsItem = {
  decoded: Record<string, string | Array<string>>;
  id: string;
  is_deleted: boolean;
  timestamp: string;
};

export type AddressMudRecordsFilter = {
  filter_key0?: string;
  filter_key1?: string;
};

export type AddressMudRecordsSorting = {
  sort: 'key0' | 'key1';
  order: 'asc' | 'desc' | undefined;
};

export type AddressMudRecord = {
  record: AddressMudRecordsItem;
  schema: MudWorldSchema;
  table: MudWorldTable;
};

export type AddressEpochRewardsResponse = {
  items: Array<AddressEpochRewardsItem>;
  next_page_params: {
    amount: string;
    associated_account_address_hash: string;
    epoch_number: number;
    items_count: number;
    type: CeloEpochRewardsType;
  } | null;
};

export type AddressEpochRewardsItem = {
  type: CeloEpochRewardsType;
  token: TokenInfo;
  amount: string;
  block_timestamp: string;
  account: AddressParam;
  epoch_number: number;
  associated_account: AddressParam;
};

export type AddressXStarResponse = {
  data: {
    level: string | null;
  };
};
