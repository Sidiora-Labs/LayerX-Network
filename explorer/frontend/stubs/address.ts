import type {
  Address,
  AddressCoinBalanceHistoryItem,
  AddressCollection,
  AddressCounters,
  AddressEpochRewardsItem,
  AddressMudTableItem,
  AddressNFT,
  AddressTabsCounters,
  AddressTokenBalance,
} from 'types/api/address';
import type { AddressesItem } from 'types/api/addresses';

import { ADDRESS_HASH, ADDRESS_PARAMS } from './addressParams';
import { MUD_SCHEMA, MUD_TABLE } from './mud';
import { TOKEN_INFO_ERC_1155, TOKEN_INFO_ERC_20, TOKEN_INFO_ERC_721, TOKEN_INFO_ERC_404, TOKEN_INSTANCE } from './token';
import { TX_HASH } from './tx';

export const ADDRESS_INFO: Address = {
  block_number_balance_updated_at: 8774377,
  coin_balance: '810941268802273085757',
  creation_transaction_hash: null,
  creator_address_hash: ADDRESS_HASH,
  creation_status: 'success',
  exchange_rate: null,
  has_logs: true,
  has_token_transfers: false,
  has_tokens: false,
  has_validated_blocks: false,
  hash: ADDRESS_HASH,
  implementations: [ { address_hash: ADDRESS_HASH, name: 'Transparent Upgradable Proxy' } ],
  is_contract: true,
  is_verified: true,
  name: 'ChainLink Token (goerli)',
  token: TOKEN_INFO_ERC_20,
  private_tags: [],
  public_tags: [],
  watchlist_names: [],
  watchlist_address_id: null,
  ens_domain_name: null,
};

export const ADDRESS_COUNTERS: AddressCounters = {
  gas_usage_count: '8028907522',
  token_transfers_count: '420',
  transactions_count: '119020',
  validations_count: '0',
};

export const ADDRESS_TABS_COUNTERS: AddressTabsCounters = {
  internal_transactions_count: 10,
  logs_count: 10,
  token_balances_count: 10,
  token_transfers_count: 10,
  transactions_count: 10,
  validations_count: 10,
  withdrawals_count: 10,
  beacon_deposits_count: 10,
};

export const TOP_ADDRESS: AddressesItem = {
  coin_balance: '11886682377162664596540805',
  transactions_count: '1835',
  hash: '0x4f7A67464B5976d7547c860109e4432d50AfB38e',
  implementations: null,
  is_contract: false,
  is_verified: null,
  name: null,
  private_tags: [],
  public_tags: [ ],
  watchlist_names: [],
  ens_domain_name: null,
};

export const ADDRESS_COIN_BALANCE: AddressCoinBalanceHistoryItem = {
  block_number: 9004413,
  block_timestamp: '2023-05-15T13:16:24Z',
  delta: '1000000000000000000',
  transaction_hash: TX_HASH,
  value: '953427250000000000000000',
};

export const ADDRESS_TOKEN_BALANCE_ERC_20: AddressTokenBalance = {
  token: TOKEN_INFO_ERC_20,
  token_id: null,
  token_instance: null,
  value: '1000000000000000000000000',
};

export const ADDRESS_NFT_721: AddressNFT = {
  token_type: 'ERC-721',
  value: '1',
  ...TOKEN_INSTANCE,
  token: TOKEN_INFO_ERC_721,
};

export const ADDRESS_NFT_1155: AddressNFT = {
  token_type: 'ERC-1155',
  value: '10',
  ...TOKEN_INSTANCE,
  token: TOKEN_INFO_ERC_1155,
};

export const ADDRESS_NFT_404: AddressNFT = {
  token_type: 'ERC-404',
  value: '10',
  ...TOKEN_INSTANCE,
  token: TOKEN_INFO_ERC_404,
};

export const ADDRESS_COLLECTION: AddressCollection = {
  token: TOKEN_INFO_ERC_1155,
  amount: '4',
  token_instances: Array(4).fill(TOKEN_INSTANCE),
};

export const ADDRESS_MUD_TABLE_ITEM: AddressMudTableItem = {
  schema: MUD_SCHEMA,
  table: MUD_TABLE,
};

export const EPOCH_REWARD_ITEM: AddressEpochRewardsItem = {
  amount: '136609473658452408568',
  block_timestamp: '2022-05-15T13:16:24Z',
  type: 'voter',
  token: TOKEN_INFO_ERC_20,
  account: ADDRESS_PARAMS,
  epoch_number: 1234,
  associated_account: ADDRESS_PARAMS,
};
