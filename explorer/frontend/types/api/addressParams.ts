import type { AddressMetadataTagApi } from './addressMetadata';
import type { SmartContractProxyType } from './contract';
import type { TokenReputation } from './token';

export interface AddressImplementation {
  address_hash: string;
  filecoin_robust_address?: string | null;
  name?: string | null;
}

export interface AddressTag {
  label: string;
  display_name: string;
  address_hash: string;
}

export interface WatchlistName {
  label: string;
  display_name: string;
}

export type AddressFilecoinParams = {
  actor_type?: FilecoinActorType;
  id?: string | null;
  robust?: string | null;
};

export type FilecoinActorType =
  'account' |
  'cron' |
  'datacap' |
  'eam' |
  'ethaccount' |
  'evm' |
  'init' |
  'market' |
  'miner' |
  'multisig' |
  'paych' |
  'placeholder' |
  'power' |
  'reward' |
  'system' |
  'verifreg';

export interface UserTags {
  private_tags: Array<AddressTag> | null;
  watchlist_names: Array<WatchlistName> | null;
  public_tags: Array<AddressTag> | null;
}

export type AddressParamBasic = {
  hash: string;
  implementations: Array<AddressImplementation> | null;
  name: string | null;
  is_contract: boolean;
  is_verified: boolean | null;
  ens_domain_name: string | null;
  metadata?: {
    reputation: number | null;
    tags: Array<AddressMetadataTagApi>;
  } | null;
  filecoin?: AddressFilecoinParams;
  proxy_type?: SmartContractProxyType | null;
  reputation?: TokenReputation;
};

export type AddressParam = UserTags & AddressParamBasic;
