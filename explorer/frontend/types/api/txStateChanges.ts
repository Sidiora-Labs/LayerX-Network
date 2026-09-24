import type { AddressParam } from './addressParams';
import type { TokenInfo } from './token';
import type { Erc721TotalPayload } from './tokenTransfer';

export type TxStateChange = (TxStateChangeCoin | TxStateChangeToken) & {
  address: AddressParam;
  is_miner: boolean;
  balance_before: string | null;
  balance_after: string | null;
};

export interface TxStateChangeCoin {
  type: 'coin';
  change: string;
  token: null;
}

export type TxStateChangeToken = TxStateChangeTokenErc20 | TxStateChangeTokenErc721 | TxStateChangeTokenErc1155;

type ChangeDirection = 'from' | 'to';

export interface TxStateChangeTokenErc20 {
  type: 'token';
  token: TokenInfo;
  change: string;
}

export interface TxStateChangeTokenErc721 {
  type: 'token';
  token: TokenInfo;
  change: Array<{
    direction: ChangeDirection;
    total: Erc721TotalPayload;
  }>;
}

export interface TxStateChangeTokenErc1155 {
  type: 'token';
  token: TokenInfo;
  change: string;
  token_id: string;
}

export interface TxStateChangeTokenErc404 {
  type: 'token';
  token: TokenInfo;
  change: string;
  token_id: string;
}

export type TxStateChanges = {
  items: Array<TxStateChange>;
  next_page_params: {
    items_count: number;
    state_changes: null;
  };
};
